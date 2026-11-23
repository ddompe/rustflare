//! Sending a ring's file descriptor to another process over a unix socket (SCM_RIGHTS), and length-prefixed messages around it.
//! The recorder (which owns the ring) and the stacker (which reads bursts from it) use this; either can restart and reconnect.

use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;

/// The largest message accepted (a guard against a corrupt stream).
pub const MAX_MESSAGE: usize = 1 << 20;

/// Sends one message (4-byte little-endian length, then the bytes), with a file descriptor attached to it if given.
pub fn send(sock: &UnixStream, body: &[u8], fd: Option<RawFd>) -> io::Result<()> {
    let mut buf = Vec::with_capacity(body.len() + 4);
    buf.extend_from_slice(&(body.len() as u32).to_le_bytes());
    buf.extend_from_slice(body);
    let mut sent = 0;
    while sent < buf.len() {
        let chunk = &buf[sent..];
        let mut iov = libc::iovec { iov_base: chunk.as_ptr() as *mut libc::c_void, iov_len: chunk.len() };
        let space = unsafe { libc::CMSG_SPACE(std::mem::size_of::<RawFd>() as u32) } as usize;
        let mut cbuf = vec![0u8; space];
        let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        if let (Some(fd), 0) = (fd, sent) {
            msg.msg_control = cbuf.as_mut_ptr() as *mut libc::c_void;
            msg.msg_controllen = space as _;
            unsafe {
                let c = libc::CMSG_FIRSTHDR(&msg);
                (*c).cmsg_level = libc::SOL_SOCKET;
                (*c).cmsg_type = libc::SCM_RIGHTS;
                (*c).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<RawFd>() as u32) as _;
                std::ptr::copy_nonoverlapping(&fd as *const RawFd as *const u8, libc::CMSG_DATA(c), std::mem::size_of::<RawFd>());
            }
        }
        let n = unsafe { libc::sendmsg(sock.as_raw_fd(), &msg, 0) };
        if n < 0 {
            let e = io::Error::last_os_error();
            if e.kind() == io::ErrorKind::Interrupted { continue }
            return Err(e);
        }
        sent += n as usize;
    }
    Ok(())
}

/// Reads messages from a socket, collecting any file descriptors that arrive with them.
pub struct Receiver {
    sock: UnixStream,
    buf: Vec<u8>,
    fds: std::collections::VecDeque<OwnedFd>,
}

impl Receiver {
    pub fn new(sock: UnixStream) -> Receiver { Receiver { sock, buf: Vec::new(), fds: Default::default() } }

    /// Blocks until a whole message is there. `Ok(None)` when the other side closed the connection.
    pub fn recv(&mut self) -> io::Result<Option<Vec<u8>>> {
        loop {
            if self.buf.len() >= 4 {
                let len = u32::from_le_bytes([self.buf[0], self.buf[1], self.buf[2], self.buf[3]]) as usize;
                if len > MAX_MESSAGE {
                    return Err(io::Error::new(io::ErrorKind::InvalidData, "message too large"));
                }
                if self.buf.len() >= 4 + len {
                    let msg = self.buf[4..4 + len].to_vec();
                    self.buf.drain(..4 + len);
                    return Ok(Some(msg));
                }
            }
            let mut tmp = [0u8; 8192];
            let mut iov = libc::iovec { iov_base: tmp.as_mut_ptr() as *mut libc::c_void, iov_len: tmp.len() };
            let space = unsafe { libc::CMSG_SPACE((4 * std::mem::size_of::<RawFd>()) as u32) } as usize;
            let mut cbuf = vec![0u8; space];
            let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
            msg.msg_iov = &mut iov;
            msg.msg_iovlen = 1;
            msg.msg_control = cbuf.as_mut_ptr() as *mut libc::c_void;
            msg.msg_controllen = space as _;
            let n = unsafe { libc::recvmsg(self.sock.as_raw_fd(), &mut msg, 0) };
            if n < 0 {
                let e = io::Error::last_os_error();
                if e.kind() == io::ErrorKind::Interrupted { continue }
                return Err(e);
            }
            unsafe {
                let mut c = libc::CMSG_FIRSTHDR(&msg);
                while !c.is_null() {
                    if (*c).cmsg_level == libc::SOL_SOCKET && (*c).cmsg_type == libc::SCM_RIGHTS {
                        let bytes = (*c).cmsg_len as usize - libc::CMSG_LEN(0) as usize;
                        let data = libc::CMSG_DATA(c) as *const RawFd;
                        for k in 0..bytes / std::mem::size_of::<RawFd>() {
                            self.fds.push_back(OwnedFd::from_raw_fd(std::ptr::read_unaligned(data.add(k))));
                        }
                    }
                    c = libc::CMSG_NXTHDR(&msg, c);
                }
            }
            if n == 0 {
                return Ok(None);
            }
            self.buf.extend_from_slice(&tmp[..n as usize]);
        }
    }

    /// The oldest file descriptor received and not yet taken.
    pub fn take_fd(&mut self) -> Option<OwnedFd> { self.fds.pop_front() }
}

/// A convenience for senders that hold the socket behind a lock: writes a whole message.
pub fn send_text(sock: &UnixStream, text: &str, fd: Option<RawFd>) -> io::Result<()> { send(sock, text.as_bytes(), fd) }

#[allow(dead_code)]
fn _assert_traits(_: &dyn Read, _: &dyn Write) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Geometry, Ring};

    #[test]
    fn a_ring_travels_to_another_mapping_with_its_messages_in_order() {
        let g = Geometry { width: 16, height: 8, bits: 16, format: 0 };
        let (ring, fd) = Ring::create(g, 4).unwrap();
        let (a, b) = UnixStream::pair().unwrap();
        send(&a, b"hello", None).unwrap();
        send(&a, b"ring", Some(std::os::fd::AsRawFd::as_raw_fd(&fd))).unwrap();
        send(&a, &vec![7u8; 5_000], None).unwrap();
        let mut rx = Receiver::new(b);
        assert_eq!(rx.recv().unwrap().unwrap(), b"hello");
        // (a descriptor may already have been received along with the bytes that follow: what matters is that it is there by the time
        // its own message has been read)
        assert_eq!(rx.recv().unwrap().unwrap(), b"ring");
        let got = rx.take_fd().expect("the descriptor arrived with its message");
        assert_eq!(rx.recv().unwrap().unwrap().len(), 5_000);
        // the other side maps the same memory
        let other = Ring::from_fd(std::os::fd::AsFd::as_fd(&got)).unwrap();
        let mut p = ring.become_producer();
        p.push(&vec![9u8; ring.frame_bytes()], Default::default()).unwrap();
        assert_eq!(other.newest(), 1);
        drop(a);
        assert!(rx.recv().unwrap().is_none(), "a closed connection reads as the end");
    }
}

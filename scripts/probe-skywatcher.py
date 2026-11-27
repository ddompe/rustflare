#!/usr/bin/env python3
"""Read-only probe of a Sky-Watcher mount's USB serial port: which protocol answers, at which baud rate.

    python3 scripts/probe-skywatcher.py /dev/ttyACM0        (Linux)
    python3 scripts/probe-skywatcher.py /dev/cu.usbmodemXXXX  (macOS)

It sends only inquiries (motor controller :e :a :b :g :f :j, and the SynScan hand controller echo/version commands). It never sends a
start, goto, set-position or motion-mode command, so it cannot move the mount. Needs only the Python 3 standard library.
Send the whole output to whoever is writing the driver.
"""
import os, select, sys, termios, time

BAUDS = [(115200, termios.B115200), (9600, termios.B9600), (57600, termios.B57600), (19200, termios.B19200)]
MOTOR = [":e1", ":e2", ":a1", ":a2", ":b1", ":g1", ":g2", ":f1", ":f2", ":j1", ":j2"]
SYNSCAN = ["Kx", "e", "V", "J"]  # echo test, RA/Dec, version, "is the alignment complete"


def open_port(path, baud):
    fd = os.open(path, os.O_RDWR | os.O_NOCTTY | os.O_NONBLOCK)
    a = termios.tcgetattr(fd)
    a[0] = 0; a[1] = 0; a[3] = 0
    a[2] = termios.CS8 | termios.CREAD | termios.CLOCAL
    a[4] = a[5] = baud
    a[6][termios.VMIN] = 0; a[6][termios.VTIME] = 0
    termios.tcsetattr(fd, termios.TCSANOW, a)
    termios.tcflush(fd, termios.TCIOFLUSH)
    return fd


def ask(fd, data, wait=0.6):
    termios.tcflush(fd, termios.TCIFLUSH)
    os.write(fd, data)
    out = b""
    end = time.time() + wait
    while time.time() < end:
        r, _, _ = select.select([fd], [], [], 0.1)
        if r:
            chunk = os.read(fd, 256)
            if chunk:
                out += chunk
                end = time.time() + 0.15
    return out


def main():
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    path = sys.argv[1]
    print(f"probing {path} (read-only inquiries only)\n")
    found = False
    for baud, const in BAUDS:
        fd = open_port(path, const)
        print(f"--- {baud} baud")
        for c in MOTOR:
            r = ask(fd, c.encode() + b"\r")
            print(f"  motor controller {c!r:8} -> {r!r}")
            found |= r.startswith(b"=")
        for c in SYNSCAN:
            r = ask(fd, c.encode())
            print(f"  hand controller  {c!r:8} -> {r!r}")
            found |= r.endswith(b"#")
        os.close(fd)
    print("\nan answer starting with '=' is the motor controller protocol; one ending with '#' is the SynScan hand controller protocol")
    print("something answered" if found else "nothing answered: wrong port, mount off, or another program (the SynScan app, EQMOD) holds the port")


if __name__ == "__main__":
    main()

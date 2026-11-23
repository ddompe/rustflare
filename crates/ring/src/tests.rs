use super::*;
use std::os::fd::AsFd;

fn geo() -> Geometry { Geometry { width: 8, height: 4, bits: 16, format: 0 } } // 64-byte frames

fn frame(i: u64) -> Vec<u8> { (0..64).map(|k| (i as usize * 7 + k) as u8).collect() }

fn meta(i: u64) -> FrameMeta { FrameMeta { ts_ns: 1000 + i, burst_id: i / 10, exposure_us: 7414, gain: 71, flags: 0 } }

#[test]
fn frames_arrive_in_order_with_their_data_and_metadata() {
    let (ring, _fd) = Ring::create(geo(), 8).unwrap();
    let mut p = ring.become_producer();
    let mut c = ring.attach_consumer("writer", true, false).unwrap();
    for i in 1..=5 {
        assert_eq!(p.push(&frame(i), meta(i)).unwrap(), i);
    }
    for i in 1..=5 {
        let f = c.peek().unwrap();
        assert_eq!((f.seq, f.data, f.ts_ns, f.burst_id, f.exposure_us, f.gain), (i, &frame(i)[..], 1000 + i, i / 10, 7414, 71));
        assert!(f.still_valid());
        let seq = f.seq;
        c.advance(seq);
    }
    assert_eq!(c.peek().err(), Some(Next::Empty));
    assert_eq!(c.backlog(), 0);
}

#[test]
fn a_must_not_lose_consumer_holds_the_producer_back_until_it_catches_up() {
    let (ring, _fd) = Ring::create(geo(), 4).unwrap();
    let mut p = ring.become_producer();
    let mut c = ring.attach_consumer("writer", true, false).unwrap();
    for i in 1..=4 { p.push(&frame(i), meta(i)).unwrap(); }
    assert_eq!(p.push(&frame(5), meta(5)).err(), Some(Full::HeldBack), "slot 1 is still needed");
    let f = c.peek().unwrap(); let s = f.seq; c.advance(s);
    assert_eq!(p.push(&frame(5), meta(5)).unwrap(), 5);
    assert_eq!(p.push(&frame(6), meta(6)).err(), Some(Full::HeldBack));
}

#[test]
fn a_best_effort_consumer_never_holds_anything_back_and_is_told_when_lapped() {
    let (ring, _fd) = Ring::create(geo(), 4).unwrap();
    let mut p = ring.become_producer();
    let mut c = ring.attach_consumer("preview", false, false).unwrap();
    for i in 1..=10 { p.push(&frame(i), meta(i)).unwrap(); }
    assert_eq!(c.peek().err(), Some(Next::Lapped(6)), "frames 1 to 6 were overwritten");
    let f = c.peek().unwrap();
    assert_eq!((f.seq, f.data), (7, &frame(7)[..]));
}

#[test]
fn a_pin_keeps_a_burst_in_the_ring_for_random_access() {
    let (ring, _fd) = Ring::create(geo(), 6).unwrap();
    let mut p = ring.become_producer();
    let c = ring.attach_consumer("stacker", true, false).unwrap();
    for i in 1..=4 { p.push(&frame(i), meta(i)).unwrap(); }
    assert!(c.pin(2));
    c.advance(4); // the cursor says it is done, the pin says it still needs 2 onwards
    assert_eq!(p.push(&frame(5), meta(5)).unwrap(), 5);
    assert_eq!(p.push(&frame(6), meta(6)).unwrap(), 6);
    assert_eq!(p.push(&frame(7), meta(7)).unwrap(), 7, "frame 7 reuses frame 1's slot: 1 is before the pin");
    assert_eq!(p.push(&frame(8), meta(8)).err(), Some(Full::HeldBack), "frame 8 would reuse frame 2's slot, which is pinned");
    // frames of the pinned range can be read in any order, repeatedly
    for seq in [6, 2, 4, 2] {
        let f = c.frame_at(seq).unwrap();
        assert_eq!(f.data, &frame(seq)[..]);
    }
    c.unpin();
    assert_eq!(p.push(&frame(8), meta(8)).unwrap(), 8);
    assert!(!c.pin(1), "frame 1 is gone: a pin cannot be taken on it");
}

#[test]
fn an_uncommitted_slot_is_invisible_and_a_new_producer_carries_on_numbering() {
    let (ring, fd) = Ring::create(geo(), 8).unwrap();
    let mut p = ring.become_producer();
    for i in 1..=3 { p.push(&frame(i), meta(i)).unwrap(); }
    {
        let mut w = p.begin().unwrap(); // the producer "dies" in the middle of frame 4
        w.data[..8].copy_from_slice(&[9; 8]);
    }
    let mut c = ring.attach_consumer("reader", true, true).unwrap();
    assert_eq!(ring.newest(), 3, "the half-written frame was never committed");
    // a second process maps the same memory and becomes the producer
    let ring2 = Ring::from_fd(fd.as_fd()).unwrap();
    let mut p2 = ring2.become_producer();
    assert_eq!(p2.push(&frame(4), meta(4)).unwrap(), 4, "numbering continues from the committed frames");
    for i in 1..=4 {
        let f = c.peek().unwrap(); assert_eq!((f.seq, f.data), (i, &frame(i)[..])); let s = f.seq; c.advance(s);
    }
}

#[test]
fn dead_consumers_are_reaped_and_stop_holding_the_producer_back() {
    let (ring, _fd) = Ring::create(geo(), 4).unwrap();
    let mut p = ring.become_producer();
    let c = ring.attach_consumer("stacker", true, false).unwrap();
    for i in 1..=4 { p.push(&frame(i), meta(i)).unwrap(); }
    assert!(c.pin(1));
    assert_eq!(p.push(&frame(5), meta(5)).err(), Some(Full::HeldBack));
    assert_eq!(ring.reap_dead(60_000_000_000), 0, "a recent heartbeat keeps it");
    std::thread::sleep(std::time::Duration::from_millis(20));
    assert_eq!(ring.reap_dead(1_000_000), 1, "a stale heartbeat does not");
    assert_eq!(p.push(&frame(5), meta(5)).unwrap(), 5);
    assert!(ring.consumers().is_empty());
}

#[test]
fn consumers_are_listed_with_their_names_and_cursors() {
    let (ring, _fd) = Ring::create(geo(), 4).unwrap();
    let mut p = ring.become_producer();
    let c1 = ring.attach_consumer("raw-writer", true, false).unwrap();
    let _c2 = ring.attach_consumer("preview", false, false).unwrap();
    p.push(&frame(1), meta(1)).unwrap();
    c1.advance(1);
    let list = ring.consumers();
    assert_eq!(list.len(), 2);
    assert_eq!((list[0].name.as_str(), list[0].must_not_lose, list[0].read_seq, list[0].pid), ("raw-writer", true, 1, std::process::id()));
    assert_eq!((list[1].name.as_str(), list[1].must_not_lose), ("preview", false));
}

#[test]
fn a_foreign_or_damaged_memory_is_refused() {
    let (_ring, fd) = Ring::create(geo(), 4).unwrap();
    // zero the magic through a second mapping
    let ring2 = Ring::from_fd(fd.as_fd()).unwrap();
    unsafe { std::ptr::write(ring2.map.ptr() as *mut u64, 0); }
    assert!(matches!(Ring::from_fd(fd.as_fd()), Err(Error::BadRing(_))));
}

/// A producer thread and a consumer thread on two separate mappings of the same memory, thousands of frames, nothing lost, duplicated,
/// reordered or torn (every byte of a frame is derived from its number).
#[test]
fn a_producer_and_a_consumer_on_separate_mappings_agree_on_every_frame() {
    let (ring, fd) = Ring::create(Geometry { width: 256, height: 16, bits: 16, format: 0 }, 16).unwrap(); // 8 KB frames, 16 slots
    let total = 20_000u64;
    let consumer_ring = Ring::from_fd(fd.as_fd()).unwrap();
    let mut c = consumer_ring.attach_consumer("reader", true, false).unwrap();
    let reader = std::thread::spawn(move || {
        let mut expect = 1u64;
        while expect <= total {
            match c.peek() {
                Ok(f) => {
                    assert_eq!(f.seq, expect);
                    let b = (f.seq % 251) as u8;
                    assert!(f.data.iter().all(|&x| x == b), "frame {} is torn", f.seq);
                    assert_eq!(f.ts_ns, f.seq * 3);
                    let s = f.seq;
                    c.advance(s);
                    expect += 1;
                }
                Err(_) => std::thread::yield_now(),
            }
        }
    });
    let mut p = ring.become_producer();
    let mut sent = 0u64;
    while sent < total {
        let i = sent + 1;
        match p.begin() {
            Ok(mut w) => {
                w.data.iter_mut().for_each(|x| *x = (i % 251) as u8);
                w.set_meta(FrameMeta { ts_ns: i * 3, ..Default::default() });
                w.commit();
                sent += 1;
            }
            Err(Full::HeldBack) => std::thread::yield_now(),
        }
    }
    reader.join().unwrap();
}

/// Lapped readers must never be handed a torn frame as a good one: after reading they can ask whether the slot still holds the frame.
#[test]
fn a_lapped_reader_can_tell_that_the_frame_it_read_was_overwritten() {
    let (ring, _fd) = Ring::create(geo(), 4).unwrap();
    let mut p = ring.become_producer();
    let mut c = ring.attach_consumer("preview", false, false).unwrap();
    for i in 1..=4 { p.push(&frame(i), meta(i)).unwrap(); }
    let f = c.peek().unwrap();
    assert!(f.still_valid());
    for i in 5..=8 { p.push(&frame(i), meta(i)).unwrap(); } // laps the whole ring while the reader holds frame 1
    assert!(!f.still_valid());
}

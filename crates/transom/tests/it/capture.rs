use transom::capture::{CaptureError, CaptureSource, FakeCapture, FrameData, PhysicalSizeWire};

const SIZE: PhysicalSizeWire = PhysicalSizeWire {
    width: 4,
    height: 2,
};

#[test]
fn a_fake_serves_its_frames_in_order_then_ends() {
    let mut cap = FakeCapture::solid(SIZE, 3);
    assert_eq!(cap.size(), SIZE);
    for n in 0..3u64 {
        let frame = cap.frame().expect("a frame");
        assert_eq!(frame.presented_ns, n * 1_000_000);
        match frame.data {
            FrameData::Shm { stride, bytes, .. } => {
                assert_eq!(stride, 16);
                assert_eq!(bytes.len(), 32);
                assert!(bytes.iter().all(|b| u64::from(*b) == n));
            }
            other => panic!("{other:?}"),
        }
    }
    assert_eq!(cap.remaining(), 0);
    assert!(matches!(cap.frame(), Err(CaptureError::Ended)));
}

#[test]
fn a_waiting_read_steps_over_a_quiet_spell_and_a_timed_read_reports_it() {
    use std::time::Duration;
    use transom::capture::Arrival;

    let frame = |n: u64| {
        FakeCapture::solid(SIZE, 1)
            .frame()
            .map(|mut f| {
                f.presented_ns = n;
                f
            })
            .expect("a frame")
    };
    let run = || vec![None, Some(frame(7)), None];

    let mut waiting = FakeCapture::scripted(SIZE, run());
    assert_eq!(
        waiting
            .frame()
            .expect("the frame after the spell")
            .presented_ns,
        7
    );
    assert_eq!(waiting.remaining(), 0);

    let mut timed = FakeCapture::scripted(SIZE, run());
    let wait = Duration::from_millis(1);
    assert!(matches!(timed.next_frame(wait), Ok(Arrival::Quiet)));
    assert!(matches!(timed.next_frame(wait), Ok(Arrival::Frame(f)) if f.presented_ns == 7));
    assert!(matches!(timed.next_frame(wait), Ok(Arrival::Quiet)));
    assert!(matches!(timed.next_frame(wait), Err(CaptureError::Ended)));
}

/// PORTAL-12: a picture's memory goes back to its pool and is used again, clean, within a bound.
#[test]
fn the_memory_of_a_picture_is_used_again_clean_and_within_a_bound() {
    use transom::capture::{FramePool, POOL_KEEP, PixelBuffer};

    let pool = FramePool::new();
    let mut first = pool.take(1000);
    first.as_mut_slice().fill(7);
    let place = first.as_ptr();
    drop(first);
    assert_eq!(pool.idle(), 1, "a dropped picture goes home");
    let second = pool.take(1000);
    assert_eq!(second.as_ptr(), place, "the same memory comes back");
    assert!(second.iter().all(|b| *b == 0), "and it is clean");
    assert_eq!(pool.idle(), 0);
    drop(second);

    let many: Vec<PixelBuffer> = (0..POOL_KEEP + 2).map(|_| pool.take(64)).collect();
    drop(many);
    assert_eq!(pool.idle(), POOL_KEEP, "the pool keeps a few and no more");

    let small = FramePool::new();
    drop(small.take(100_000));
    let shrunk = small.take(100);
    assert_eq!(shrunk.len(), 100);
    assert_eq!(small.idle(), 0, "memory far larger than needed is let go");

    drop(PixelBuffer::from(vec![1, 2, 3]));
    assert_eq!(pool.idle(), POOL_KEEP, "a buffer of no pool joins none");
}

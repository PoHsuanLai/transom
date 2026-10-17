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

/// NEW-FA-2: the session's display is first asked of the compositor as a connection of the
/// portal's own, and the connection it hands out is the one used.
#[test]
fn a_connection_the_compositor_hands_out_is_the_way_in() {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;
    use std::sync::Arc;
    use transom::capture::{DisplayName, FakeLine, Route};

    let line = Arc::new(FakeLine::granting());
    let display = DisplayName::session().with_portal_line(line.clone());
    let Route::Handed(fd) = display.route() else {
        panic!("a granted connection is used");
    };
    let mut client = UnixStream::from(fd);
    let mut server = line.take_server().expect("the compositor's end");
    client.write_all(b"hello").expect("write");
    let mut heard = [0_u8; 5];
    server.read_exact(&mut heard).expect("read");
    assert_eq!(&heard, b"hello");
    assert_eq!(line.asked(), 1);
}

/// (case, the line, how many times it is asked). Whatever the reason, the capture falls back to the
/// session's own display, as it did before the compositor handed out connections.
#[test]
fn no_connection_from_the_compositor_falls_back_to_the_sessions_display() {
    use std::sync::Arc;
    use transom::capture::{DisplayName, FakeLine, Route};

    let cases = [
        ("nobody owns the name", FakeLine::unowned()),
        ("the compositor refuses", FakeLine::refusing()),
        ("the compositor does not answer", FakeLine::silent()),
    ];
    for (case, line) in cases {
        let line = Arc::new(line);
        let display = DisplayName::session().with_portal_line(line.clone());
        for _ in 0..2 {
            assert!(matches!(display.route(), Route::Env), "{case}");
        }
        assert_eq!(line.asked(), 2, "{case}: asked for each connection");
    }
    assert!(matches!(DisplayName::session().route(), Route::Env));
}

/// A display named by its socket path is used as given; the compositor is not asked.
#[test]
fn a_display_named_by_path_is_not_asked_of_the_compositor() {
    use std::sync::Arc;
    use transom::capture::{DisplayName, FakeLine, Route};

    let line = Arc::new(FakeLine::granting());
    let display = DisplayName::at("/run/user/1000/wayland-9").with_portal_line(line.clone());
    match display.route() {
        Route::Path(path) => assert_eq!(path, "/run/user/1000/wayland-9"),
        other => panic!("{other:?}"),
    }
    assert_eq!(line.asked(), 0);
}

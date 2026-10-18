use transom::capture::{CaptureError, CaptureSource, FakeCapture, FrameData};
use transom_core::Extent;

const SIZE: Extent = Extent {
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

/// A compositor with nothing to offer: it answers every round trip and ignores the rest, which is
/// enough for a job to connect, list no outputs and find no capture protocols.
fn answer_round_trips(mut server: std::os::unix::net::UnixStream) {
    use std::io::{Read, Write};

    let mut head = [0_u8; 8];
    while server.read_exact(&mut head).is_ok() {
        let word =
            |at: usize| u32::from_ne_bytes([head[at], head[at + 1], head[at + 2], head[at + 3]]);
        let (object, size, opcode) = (
            word(0),
            usize::try_from(word(4) >> 16).unwrap_or(0),
            word(4) & 0xffff,
        );
        let mut body = vec![0_u8; size.saturating_sub(8)];
        if server.read_exact(&mut body).is_err() {
            return;
        }
        // `wl_display.sync(callback)`: the callback is told it is done.
        if object == 1 && opcode == 0 && body.len() >= 4 {
            let mut done = body[..4].to_vec();
            done.extend_from_slice(&(12_u32 << 16).to_ne_bytes());
            done.extend_from_slice(&0_u32.to_ne_bytes());
            if server.write_all(&done).is_err() {
                return;
            }
        }
    }
}

/// A display whose line hands out connections to a compositor that has nothing to offer.
fn display_behind_a_line() -> (
    std::sync::Arc<transom::capture::FakeLine>,
    transom::capture::DisplayName,
) {
    use std::sync::Arc;
    use std::time::{Duration, Instant};
    use transom::capture::{DisplayName, FakeLine};

    let line = Arc::new(FakeLine::granting());
    let waiting = Arc::clone(&line);
    std::thread::spawn(move || {
        let give_up = Instant::now() + Duration::from_secs(10);
        while Instant::now() < give_up {
            if let Some(server) = waiting.take_server() {
                return answer_round_trips(server);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    });
    let display = DisplayName::session().with_portal_line(line.clone());
    (line, display)
}

/// NEW-FA-2 follow-up: the compositor limits how often a caller may ask for a connection, so a job
/// (a screenshot's look at the outputs and a capture of each, a share's lookups and streams) asks
/// once, when it first needs one, and every step runs on what it got.
#[test]
fn every_step_of_a_job_runs_on_the_one_connection_the_line_hands_out() {
    use transom::capture::{CaptureError, ExtCapture, Job};
    use transom_core::CastSource;

    let (line, display) = display_behind_a_line();
    let job = Job::new(&display);
    assert_eq!(line.asked(), 0, "nothing is asked before the first step");
    assert!(job.output_facts().expect("the outputs").is_empty());
    assert!(job.live_sources().expect("the sources").is_empty());
    for name in ["A", "B", "C"] {
        let source = CastSource::Output(name.to_owned());
        assert!(matches!(
            ExtCapture::open(&job, &source),
            Err(CaptureError::Unsupported)
        ));
    }
    assert_eq!(line.asked(), 1, "one connection for the five steps");
}

/// A screen-cast start with two streams: the lookups and both streams are one job.
#[test]
fn a_share_of_two_streams_asks_the_line_once() {
    use transom::capture::{CaptureError, Job};
    use transom::screencast::run::{OpenError, Share};
    use transom_core::{CastSource, CursorMode};

    let (line, display) = display_behind_a_line();
    let job = Job::new(&display);
    assert!(job.live_sources().expect("the sources").is_empty());
    let sources = [
        CastSource::Output("A".to_owned()),
        CastSource::Window("w".to_owned()),
    ];
    let opened = Share::open(&job, &sources, CursorMode::Hidden, || {});
    assert!(matches!(
        opened,
        Err(OpenError::Capture(CaptureError::Unsupported))
    ));
    assert_eq!(line.asked(), 1);
}

/// Whether a program with no display of its own would find the session's public socket anyway.
fn public_display_may_exist() -> bool {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from);
    std::env::var_os("WAYLAND_DISPLAY").is_some()
        || std::env::var_os("WAYLAND_SOCKET").is_some()
        || runtime.is_some_and(|dir| dir.join("wayland-0").exists())
}

/// Where the compositor hands out no connection, the job falls back to the session's public
/// socket once, not once per step.
#[test]
fn a_refused_line_sends_the_whole_job_to_the_public_socket_once() {
    use std::sync::Arc;
    use transom::capture::{CaptureError, DisplayName, FakeLine, Job};

    if public_display_may_exist() {
        eprintln!("skipped: a public Wayland display may be reachable, and tests do not use it");
        return;
    }
    let line = Arc::new(FakeLine::refusing());
    let job = Job::new(&DisplayName::session().with_portal_line(line.clone()));
    for _ in 0..3 {
        assert!(matches!(
            job.output_facts(),
            Err(CaptureError::Connection(_))
        ));
    }
    assert_eq!(line.asked(), 1, "the refusal is decided once for the job");
}

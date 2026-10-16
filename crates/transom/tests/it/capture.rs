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

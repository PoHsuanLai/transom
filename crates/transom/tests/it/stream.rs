use transom::capture::{CapturedFrame, FakeCapture, FrameData, PhysicalSizeWire, PixelFormat};
use transom::screencast::stream::{FakeSink, Pump, PumpOutcome};
use transom_core::{CursorMode, PwNode};

fn size(w: u32, h: u32) -> PhysicalSizeWire {
    PhysicalSizeWire {
        width: w,
        height: h,
    }
}

fn frame(s: PhysicalSizeWire, ns: u64) -> CapturedFrame {
    CapturedFrame {
        size: s,
        data: FrameData::Shm {
            format: PixelFormat::Xrgb8888,
            stride: s.width * 4,
            bytes: vec![0; (s.width * s.height * 4) as usize],
        },
        presented_ns: ns,
    }
}

#[test]
fn frames_reach_the_sink_and_the_end_is_reported() {
    let mut pump = Pump::new(
        FakeCapture::solid(size(4, 2), 2),
        FakeSink::new(PwNode(9)),
        CursorMode::Hidden,
    );
    assert_eq!(pump.node(), PwNode(9));
    assert_eq!(pump.turn().expect("turn"), PumpOutcome::Pushed);
    assert_eq!(pump.turn().expect("turn"), PumpOutcome::Pushed);
    assert_eq!(pump.turn().expect("turn"), PumpOutcome::SourceEnded);
    let sink = pump.into_sink();
    assert_eq!(sink.received, vec![0, 1_000_000]);
    assert!(sink.resizes.is_empty());
}

#[test]
fn a_changed_size_resizes_the_sink_first() {
    let frames = vec![
        frame(size(4, 2), 1),
        frame(size(8, 4), 2),
        frame(size(8, 4), 3),
    ];
    let mut pump = Pump::new(
        FakeCapture::new(size(4, 2), frames),
        FakeSink::new(PwNode(1)),
        CursorMode::Embedded,
    );
    let outcomes: Vec<_> = (0..3).map(|_| pump.turn().expect("turn")).collect();
    assert_eq!(
        outcomes,
        vec![
            PumpOutcome::Pushed,
            PumpOutcome::ResizedAndPushed,
            PumpOutcome::Pushed
        ]
    );
    let sink = pump.into_sink();
    assert_eq!(sink.resizes, vec![size(8, 4)]);
    assert_eq!(sink.received, vec![1, 2, 3]);
}

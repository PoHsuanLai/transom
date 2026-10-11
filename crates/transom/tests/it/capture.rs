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

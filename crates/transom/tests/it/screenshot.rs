use transom::capture::{CapturedFrame, FakeCapture, FrameData, PhysicalSizeWire, PixelFormat};
use transom::screenshot::{
    FakeOpen, FixedLayout, OutputGrab, OutputPlacement, Placed, ScreenshotSetup, ShotError,
    compose, file_uri, take,
};

fn size(width: u32, height: u32) -> PhysicalSizeWire {
    PhysicalSizeWire { width, height }
}

fn place(name: &str, x: i32, y: i32) -> OutputPlacement {
    OutputPlacement {
        name: name.into(),
        x,
        y,
    }
}

fn solid(size: PhysicalSizeWire, format: PixelFormat, pixel: [u8; 4]) -> CapturedFrame {
    CapturedFrame {
        size,
        data: FrameData::Shm {
            format,
            stride: size.width * 4,
            bytes: pixel.repeat((size.width * size.height) as usize),
        },
        presented_ns: 0,
    }
}

#[test]
fn two_outputs_side_by_side_make_one_canvas() {
    let shots = [
        Placed {
            x: 0,
            y: 0,
            frame: solid(size(2, 2), PixelFormat::Xrgb8888, [10, 20, 30, 0]),
        },
        Placed {
            x: 2,
            y: 1,
            frame: solid(size(2, 1), PixelFormat::Xbgr8888, [10, 20, 30, 0]),
        },
    ];
    let canvas = compose(&shots).expect("compose");
    assert_eq!((canvas.width, canvas.height), (4, 2));
    // Xrgb is blue, green, red in memory; Xbgr is red, green, blue. Both are opaque.
    assert_eq!(&canvas.rgba[0..4], &[30, 20, 10, 255]);
    assert_eq!(
        &canvas.rgba[(4 + 2) * 4..(4 + 2) * 4 + 4],
        &[10, 20, 30, 255]
    );
    // The corner no output covers stays empty.
    assert_eq!(&canvas.rgba[2 * 4..2 * 4 + 4], &[0, 0, 0, 0]);
}

#[test]
fn bad_input_is_refused() {
    assert!(matches!(compose(&[]), Err(ShotError::NoOutputs)));
    let short = Placed {
        x: 0,
        y: 0,
        frame: CapturedFrame {
            size: size(4, 4),
            data: FrameData::Shm {
                format: PixelFormat::Argb8888,
                stride: 16,
                bytes: vec![0; 8],
            },
            presented_ns: 0,
        },
    };
    assert!(matches!(compose(&[short]), Err(ShotError::BadFrame)));
}

#[test]
fn addresses_are_percent_encoded_and_absolute() {
    let table = [
        ("/home/p/Pictures/a.png", "file:///home/p/Pictures/a.png"),
        (
            "/home/p/My Pictures/é.png",
            "file:///home/p/My%20Pictures/%C3%A9.png",
        ),
    ];
    for (path, want) in table {
        assert_eq!(file_uri(std::path::Path::new(path)).expect("uri"), want);
    }
    assert!(matches!(
        file_uri(std::path::Path::new("a.png")),
        Err(ShotError::RelativePath)
    ));
}

#[test]
fn a_screenshot_is_a_new_png_in_the_pictures_folder() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pictures = dir.path().join("Pictures");
    let opener = FakeOpen::new()
        .with("A", FakeCapture::solid(size(8, 4), 1))
        .with("B", FakeCapture::solid(size(8, 4), 1));
    let layout = FixedLayout(vec![place("A", 0, 0), place("B", 8, 0)]);
    let setup = ScreenshotSetup::new(
        Box::new(OutputGrab::new(Box::new(layout), Box::new(opener))),
        pictures.clone(),
        "t".into(),
    );
    let path = take(&setup, 0).expect("take");
    assert_eq!(path.parent(), Some(pictures.as_path()));
    let bytes = std::fs::read(&path).expect("read");
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    // The header chunk holds the width and the height, big-endian.
    assert_eq!(&bytes[16..20], &16u32.to_be_bytes());
    assert_eq!(&bytes[20..24], &4u32.to_be_bytes());
}

#[test]
fn an_unconfigured_service_fails_and_writes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert!(take(&ScreenshotSetup::unconfigured(), 0).is_err());
    assert_eq!(std::fs::read_dir(dir.path()).expect("dir").count(), 0);
}

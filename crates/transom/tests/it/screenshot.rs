use std::sync::Arc;

use transom::access::{Answer, FakeAsker};
use transom::capture::{CapturedFrame, FakeCapture, FrameData, PhysicalSizeWire, PixelFormat};
use transom::screencast::Dict;
use transom::screenshot::{
    FakeColorPicker, FakeOpen, FixedLayout, OutputGrab, OutputPlacement, Placed, ScreenshotService,
    ScreenshotSetup, ShotError, color_from_wire, compose, file_uri, take,
};
use transom::{BoxFuture, chooser::ChooserError};
use transom_core::{Extent, OutputFacts, Point, Rgb, Turn, arrange};

fn size(width: u32, height: u32) -> PhysicalSizeWire {
    PhysicalSizeWire { width, height }
}

fn place(name: &str, x: i32, y: i32, covers: PhysicalSizeWire) -> OutputPlacement {
    OutputPlacement {
        name: name.into(),
        x,
        y,
        size: covers,
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
        Placed::unscaled(
            0,
            0,
            solid(size(2, 2), PixelFormat::Xrgb8888, [10, 20, 30, 0]),
        ),
        Placed::unscaled(
            2,
            1,
            solid(size(2, 1), PixelFormat::Xbgr8888, [10, 20, 30, 0]),
        ),
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
    let short = Placed::unscaled(
        0,
        0,
        CapturedFrame {
            size: size(4, 4),
            data: FrameData::Shm {
                format: PixelFormat::Argb8888,
                stride: 16,
                bytes: vec![0; 8],
            },
            presented_ns: 0,
        },
    );
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
    let layout = FixedLayout(vec![
        place("A", 0, 0, size(8, 4)),
        place("B", 8, 0, size(8, 4)),
    ]);
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

/// The canvas pixel at (`x`, `y`) of a canvas `width` wide.
fn pixel(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * width + x) * 4) as usize;
    [rgba[at], rgba[at + 1], rgba[at + 2], rgba[at + 3]]
}

#[test]
fn a_frame_is_stretched_to_the_place_it_covers() {
    // A one-pixel frame enlarged to 2x2, beside a 2x2 frame that already fits.
    let mut shots = vec![Placed {
        x: 0,
        y: 0,
        size: size(2, 2),
        frame: solid(size(1, 1), PixelFormat::Xbgr8888, [1, 2, 3, 0]),
    }];
    shots.push(Placed::unscaled(
        2,
        0,
        solid(size(2, 2), PixelFormat::Xbgr8888, [9, 8, 7, 0]),
    ));
    let canvas = compose(&shots).expect("compose");
    assert_eq!((canvas.width, canvas.height), (4, 2));
    for (x, y, want) in [
        (0, 0, [1, 2, 3, 255]),
        (1, 1, [1, 2, 3, 255]),
        (3, 1, [9, 8, 7, 255]),
    ] {
        assert_eq!(pixel(&canvas.rgba, 4, x, y), want, "({x}, {y})");
    }
    // A frame larger than its place is shrunk the same way.
    let shrunk = [Placed {
        x: 0,
        y: 0,
        size: size(1, 1),
        frame: solid(size(4, 4), PixelFormat::Xbgr8888, [5, 5, 5, 0]),
    }];
    let canvas = compose(&shrunk).expect("compose");
    assert_eq!((canvas.width, canvas.height), (1, 1));
}

#[test]
fn a_mixed_sharpness_pair_of_screens_becomes_one_picture() {
    // A 1x screen of 8x4 pixels beside a 2x screen of 16x8 pixels: both cover 8x4 logical
    // units, so the picture is at the 2x scale and the 1x screen is enlarged.
    let facts = [
        OutputFacts {
            name: "A".into(),
            position: Point { x: 0, y: 0 },
            mode: Extent {
                width: 8,
                height: 4,
            },
            turn: Turn::Upright,
            scale: 1,
        },
        OutputFacts {
            name: "B".into(),
            position: Point { x: 8, y: 0 },
            mode: Extent {
                width: 16,
                height: 8,
            },
            turn: Turn::Upright,
            scale: 2,
        },
    ];
    let layout = FixedLayout(
        arrange(&facts)
            .into_iter()
            .map(OutputPlacement::from)
            .collect(),
    );
    let opener = FakeOpen::new()
        .with("A", FakeCapture::solid(size(8, 4), 1))
        .with("B", FakeCapture::solid(size(16, 8), 1));
    let dir = tempfile::tempdir().expect("tempdir");
    let setup = ScreenshotSetup::new(
        Box::new(OutputGrab::new(Box::new(layout), Box::new(opener))),
        dir.path().to_path_buf(),
        "t".into(),
    );
    let path = take(&setup, 0).expect("take");
    let bytes = std::fs::read(path).expect("read");
    assert_eq!(&bytes[16..20], &32u32.to_be_bytes());
    assert_eq!(&bytes[20..24], &8u32.to_be_bytes());
}

fn ask<'a>(
    picker: &'a FakeColorPicker,
    label: &'a str,
) -> BoxFuture<'a, Result<Option<Rgb>, ChooserError>> {
    use transom::screenshot::ColorPicker;
    picker.pick(label)
}

#[tokio::test]
async fn the_colour_picker_answers_in_order_then_cancels() {
    let red = Rgb {
        r: 1.0,
        g: 0.0,
        b: 0.0,
    };
    let picker = FakeColorPicker::new(vec![Some(red)]);
    assert_eq!(ask(&picker, "Paint").await.expect("answer"), Some(red));
    assert_eq!(ask(&picker, "Paint").await.expect("answer"), None);
    assert_eq!(picker.asked(), vec!["Paint".to_owned(); 2]);
}

#[test]
fn the_shells_colour_is_one_tuple_in_range_or_a_cancel() {
    let ok = color_from_wire(&[(0.0, 0.5, 1.0)]).expect("a colour");
    assert_eq!(
        ok,
        Some(Rgb {
            r: 0.0,
            g: 0.5,
            b: 1.0
        })
    );
    assert_eq!(color_from_wire(&[]).expect("a cancel"), None);
    for bad in [
        vec![(1.5, 0.0, 0.0)],
        vec![(0.0, -0.1, 0.0)],
        vec![(f64::NAN, 0.0, 0.0)],
        vec![(0.0, 0.0, 0.0), (1.0, 1.0, 1.0)],
    ] {
        assert!(
            matches!(color_from_wire(&bad), Err(ChooserError::BadAnswer)),
            "{bad:?}"
        );
    }
}

fn flags(interactive: Option<bool>, checked: Option<bool>) -> Dict {
    let mut options = Dict::new();
    for (key, value) in [
        ("interactive", interactive),
        ("permission_store_checked", checked),
    ] {
        if let Some(value) = value {
            options.insert(key.to_owned(), zbus::zvariant::OwnedValue::from(value));
        }
    }
    options
}

#[tokio::test]
async fn a_screenshot_asks_the_person_unless_the_frontend_already_did() {
    // (interactive, permission_store_checked, the person's answers, response, pictures, asked)
    type Case = (Option<bool>, Option<bool>, Vec<Answer>, u32, usize, usize);
    let table: Vec<Case> = vec![
        // The frontend asked and said so: no second question.
        (None, Some(true), vec![], 0, 1, 0),
        (Some(false), Some(true), vec![], 0, 1, 0),
        // An interactive request is the backend's to ask, even with the frontend's flag set.
        (Some(true), Some(true), vec![Answer::Granted], 0, 1, 1),
        (Some(true), Some(true), vec![Answer::Denied], 1, 0, 1),
        (Some(true), None, vec![Answer::Granted], 0, 1, 1),
        // A request that never went through the frontend's question is asked about too.
        (None, None, vec![Answer::Denied], 1, 0, 1),
        (None, None, vec![Answer::Dismissed], 1, 0, 1),
        (Some(false), None, vec![Answer::Granted], 0, 1, 1),
        (None, Some(false), vec![Answer::Denied], 1, 0, 1),
    ];
    for (n, (interactive, checked, answers, want, pictures, asked)) in table.into_iter().enumerate()
    {
        let dir = tempfile::tempdir().expect("tempdir");
        let asker = Arc::new(FakeAsker::new(answers));
        let setup = ScreenshotSetup::new(
            Box::new(OutputGrab::new(
                Box::new(FixedLayout(vec![place("A", 0, 0, size(4, 2))])),
                Box::new(FakeOpen::new().with("A", FakeCapture::solid(size(4, 2), 1))),
            )),
            dir.path().join("Pictures"),
            "t".into(),
        )
        .with_asker(Box::new(asker.clone()));
        let service = ScreenshotService::with_setup(setup);
        let (code, results) = service
            .shoot("org.example.Grab", &flags(interactive, checked))
            .await;
        assert_eq!(code, want, "case {n}");
        assert_eq!(results.contains_key("uri"), want == 0, "case {n}");
        let made = std::fs::read_dir(dir.path().join("Pictures")).map_or(0, Iterator::count);
        assert_eq!(made, pictures, "case {n}: pictures");
        assert_eq!(asker.asked().len(), asked, "case {n}: questions");
    }
}

#[tokio::test]
async fn with_no_shell_to_ask_an_interactive_screenshot_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let setup = ScreenshotSetup::new(
        Box::new(OutputGrab::new(
            Box::new(FixedLayout(vec![place("A", 0, 0, size(4, 2))])),
            Box::new(FakeOpen::new().with("A", FakeCapture::solid(size(4, 2), 1))),
        )),
        dir.path().join("Pictures"),
        "t".into(),
    );
    let service = ScreenshotService::with_setup(setup);
    let (code, _) = service
        .shoot("org.example.Grab", &flags(Some(true), Some(true)))
        .await;
    assert_eq!(code, 1);
    assert!(!dir.path().join("Pictures").exists());
}

#[test]
fn a_screenshot_is_private_and_whole_under_its_final_name() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().expect("tempdir");
    let setup = ScreenshotSetup::new(
        Box::new(OutputGrab::new(
            Box::new(FixedLayout(vec![place("A", 0, 0, size(4, 2))])),
            Box::new(FakeOpen::new().with("A", FakeCapture::solid(size(4, 2), 1))),
        )),
        dir.path().join("Pictures"),
        "t".into(),
    );
    let path = take(&setup, 0).expect("take");
    let mode = std::fs::metadata(&path)
        .expect("metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600, "readable by its owner only");
    let names: Vec<_> = std::fs::read_dir(dir.path().join("Pictures"))
        .expect("dir")
        .map(|e| e.expect("entry").file_name())
        .collect();
    assert_eq!(names.len(), 1, "no partial file is left: {names:?}");
}

#[test]
fn a_taken_name_is_skipped_and_never_overwritten() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pictures = dir.path().join("Pictures");
    std::fs::create_dir_all(&pictures).expect("dir");
    std::fs::write(pictures.join("Screenshot-t-0.png"), b"mine").expect("write");
    // A leftover of a crashed run sits where the next name's partial file would go.
    std::fs::write(pictures.join("Screenshot-t-1.png.part"), b"stale").expect("write");
    let setup = ScreenshotSetup::new(
        Box::new(OutputGrab::new(
            Box::new(FixedLayout(vec![place("A", 0, 0, size(4, 2))])),
            Box::new(FakeOpen::new().with("A", FakeCapture::solid(size(4, 2), 1))),
        )),
        pictures.clone(),
        "t".into(),
    );
    let path = take(&setup, 0).expect("take");
    assert_eq!(path, pictures.join("Screenshot-t-2.png"));
    assert_eq!(
        std::fs::read(pictures.join("Screenshot-t-0.png")).expect("read"),
        b"mine"
    );
    assert_eq!(
        std::fs::read(pictures.join("Screenshot-t-1.png.part")).expect("read"),
        b"stale",
        "somebody else's partial file is left alone"
    );
}

//! `org.freedesktop.impl.portal.Screenshot`: a picture of the screen, and a colour picked from it.
//!
//! A screenshot grabs one frame of every output through the [`ScreenGrab`] seam, lays the frames
//! out on one canvas, encodes a PNG and writes it into the pictures folder the service was given.
//! Only this file names the image encoder.

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use zbus::zvariant::{ObjectPath, OwnedValue, Value};

use crate::capture::{
    CaptureError, CaptureSource, CapturedFrame, DisplayName, ExtCapture, FrameData, PixelFormat,
};
use crate::screencast::Dict;

/// Why a screenshot could not be made.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ShotError {
    /// No screenshots were set up, or there is no output to capture.
    #[error("there is no screen to capture")]
    NoOutputs,
    /// An output could not be captured.
    #[error("capture: {0}")]
    Capture(#[from] CaptureError),
    /// A frame arrived in a form this file cannot read (a GPU buffer).
    #[error("a frame came as a GPU buffer, which a screenshot cannot read")]
    Unreadable,
    /// A frame's size and bytes disagree.
    #[error("a frame's bytes do not match its size")]
    BadFrame,
    /// The screens together are larger than a picture can be.
    #[error("the screens together are too large for one picture")]
    TooLarge,
    /// The PNG could not be encoded.
    #[error("encoding: {0}")]
    Encode(String),
    /// The picture could not be written.
    #[error("writing the picture: {0}")]
    Write(#[from] std::io::Error),
    /// The pictures folder is not an absolute path, so the picture has no address.
    #[error("the pictures folder must be an absolute path")]
    RelativePath,
}

/// Where one output sits on the canvas, in pixels (the canvas's origin is the top-left of the
/// outputs together, so negative coordinates are fine).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputPlacement {
    /// The output's name, as the capture opens it.
    pub name: String,
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
}

/// The seam: which outputs exist and where they sit.
pub trait OutputLayout: Send + Sync + std::fmt::Debug {
    /// Every output, with its place.
    fn outputs(&self) -> Result<Vec<OutputPlacement>, ShotError>;
}

/// A layout given up front.
#[derive(Clone, Debug, Default)]
pub struct FixedLayout(pub Vec<OutputPlacement>);

impl OutputLayout for FixedLayout {
    fn outputs(&self) -> Result<Vec<OutputPlacement>, ShotError> {
        Ok(self.0.clone())
    }
}

/// The seam: how to start capturing one named output.
pub trait OpenCapture: Send + Sync + std::fmt::Debug {
    /// A source for the output called `name`.
    fn open(&self, name: &str) -> Result<Box<dyn CaptureSource>, CaptureError>;
}

/// The real opener: the compositor's image-capture protocols on a display.
#[derive(Clone, Debug)]
pub struct ExtOpen(pub DisplayName);

impl OpenCapture for ExtOpen {
    fn open(&self, name: &str) -> Result<Box<dyn CaptureSource>, CaptureError> {
        let source = transom_core::CastSource::Output(name.to_owned());
        Ok(Box::new(ExtCapture::open(&self.0, &source)?))
    }
}

/// An opener for tests: serves the sources it was given, each once.
#[derive(Debug, Default)]
pub struct FakeOpen {
    sources: std::sync::Mutex<std::collections::HashMap<String, Box<dyn CaptureSource>>>,
}

impl FakeOpen {
    /// An opener with no outputs.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add the output `name`, served by `source`.
    pub fn with(self, name: &str, source: impl CaptureSource + 'static) -> Self {
        self.sources
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(name.to_owned(), Box::new(source));
        self
    }
}

impl OpenCapture for FakeOpen {
    fn open(&self, name: &str) -> Result<Box<dyn CaptureSource>, CaptureError> {
        self.sources
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(name)
            .ok_or(CaptureError::Ended)
    }
}

/// A frame and the place of its output.
#[derive(Debug)]
pub struct Placed {
    /// Left edge on the canvas.
    pub x: i32,
    /// Top edge on the canvas.
    pub y: i32,
    /// The frame.
    pub frame: CapturedFrame,
}

/// The seam: one frame of every output.
pub trait ScreenGrab: Send + Sync + std::fmt::Debug {
    /// Grab the screens.
    fn grab(&self) -> Result<Vec<Placed>, ShotError>;
}

/// Grabs every output of a layout through an opener.
#[derive(Debug)]
pub struct OutputGrab {
    layout: Box<dyn OutputLayout>,
    opener: Box<dyn OpenCapture>,
}

impl OutputGrab {
    /// A grab of the outputs `layout` lists, opened by `opener`.
    pub fn new(layout: Box<dyn OutputLayout>, opener: Box<dyn OpenCapture>) -> Self {
        Self { layout, opener }
    }
}

impl ScreenGrab for OutputGrab {
    fn grab(&self) -> Result<Vec<Placed>, ShotError> {
        self.layout
            .outputs()?
            .into_iter()
            .map(|place| {
                let frame = self.opener.open(&place.name)?.frame()?;
                Ok::<_, ShotError>(Placed {
                    x: place.x,
                    y: place.y,
                    frame,
                })
            })
            .collect()
    }
}

/// The grab of a service nobody set up: it has no screens.
#[derive(Debug)]
struct Unconfigured;

impl ScreenGrab for Unconfigured {
    fn grab(&self) -> Result<Vec<Placed>, ShotError> {
        Err(ShotError::NoOutputs)
    }
}

/// What a service needs to take screenshots: the grab, the folder and a stamp for file names.
#[derive(Debug)]
pub struct ScreenshotSetup {
    grab: Box<dyn ScreenGrab>,
    pictures: PathBuf,
    stamp: String,
}

impl ScreenshotSetup {
    /// Screenshots from `grab`, written into the absolute folder `pictures`; `stamp` (a date and
    /// time the caller drew) goes into every file name.
    pub fn new(grab: Box<dyn ScreenGrab>, pictures: PathBuf, stamp: String) -> Self {
        Self {
            grab,
            pictures,
            stamp,
        }
    }

    /// A setup with no screens: every screenshot fails.
    pub fn unconfigured() -> Self {
        Self::new(Box::new(Unconfigured), PathBuf::new(), String::new())
    }
}

/// The canvas the frames are laid on, as 8-bit RGBA rows.
#[derive(Debug, PartialEq, Eq)]
pub struct Canvas {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

/// The most pixels a screenshot may have.
const MAX_PIXELS: u64 = 1 << 28;

/// Lay `shots` out on one opaque canvas at their places.
pub fn compose(shots: &[Placed]) -> Result<Canvas, ShotError> {
    let left = shots.iter().map(|s| i64::from(s.x)).min();
    let top = shots.iter().map(|s| i64::from(s.y)).min();
    let (Some(left), Some(top)) = (left, top) else {
        return Err(ShotError::NoOutputs);
    };
    let right = shots
        .iter()
        .map(|s| i64::from(s.x) + i64::from(s.frame.size.width))
        .max()
        .unwrap_or(left);
    let bottom = shots
        .iter()
        .map(|s| i64::from(s.y) + i64::from(s.frame.size.height))
        .max()
        .unwrap_or(top);
    let width = u32::try_from(right - left).map_err(|_| ShotError::TooLarge)?;
    let height = u32::try_from(bottom - top).map_err(|_| ShotError::TooLarge)?;
    let pixels = u64::from(width) * u64::from(height);
    if pixels == 0 || pixels > MAX_PIXELS {
        return Err(ShotError::TooLarge);
    }
    let mut rgba = vec![0u8; usize::try_from(pixels * 4).map_err(|_| ShotError::TooLarge)?];
    for shot in shots {
        let origin = (
            usize::try_from(i64::from(shot.x) - left).map_err(|_| ShotError::TooLarge)?,
            usize::try_from(i64::from(shot.y) - top).map_err(|_| ShotError::TooLarge)?,
        );
        paint(&mut rgba, width as usize, origin, &shot.frame)?;
    }
    Ok(Canvas {
        width,
        height,
        rgba,
    })
}

/// Copy one frame onto the canvas at `origin`, as opaque RGBA.
fn paint(
    canvas: &mut [u8],
    canvas_width: usize,
    origin: (usize, usize),
    frame: &CapturedFrame,
) -> Result<(), ShotError> {
    let FrameData::Shm {
        format,
        stride,
        bytes,
    } = &frame.data
    else {
        return Err(ShotError::Unreadable);
    };
    let (w, h, stride) = (
        frame.size.width as usize,
        frame.size.height as usize,
        *stride as usize,
    );
    if stride < w * 4 || bytes.len() < stride * h {
        return Err(ShotError::BadFrame);
    }
    // Memory order of the channels in each 4-byte pixel: where red, green and blue sit.
    let [r, g, b] = match format {
        PixelFormat::Argb8888 | PixelFormat::Xrgb8888 => [2, 1, 0],
        PixelFormat::Abgr8888 | PixelFormat::Xbgr8888 => [0, 1, 2],
    };
    for row in 0..h {
        let source = &bytes[row * stride..row * stride + w * 4];
        let start = ((origin.1 + row) * canvas_width + origin.0) * 4;
        let target = &mut canvas[start..start + w * 4];
        for (to, from) in target
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(source.as_chunks::<4>().0)
        {
            *to = [from[r], from[g], from[b], 255];
        }
    }
    Ok(())
}

/// A PNG of the canvas.
pub fn encode_png(canvas: Canvas) -> Result<Vec<u8>, ShotError> {
    let Canvas {
        width,
        height,
        rgba,
    } = canvas;
    let picture = image::RgbaImage::from_raw(width, height, rgba)
        .ok_or_else(|| ShotError::Encode("the canvas has the wrong number of bytes".into()))?;
    let mut out = Cursor::new(Vec::new());
    picture
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| ShotError::Encode(e.to_string()))?;
    Ok(out.into_inner())
}

/// The `file://` address of an absolute path, with every byte that is not plain percent-encoded.
pub fn file_uri(path: &Path) -> Result<String, ShotError> {
    if !path.is_absolute() {
        return Err(ShotError::RelativePath);
    }
    let mut uri = String::from("file://");
    for &byte in path.as_os_str().as_encoded_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                uri.push(char::from(byte));
            }
            other => uri.push_str(&format!("%{other:02X}")),
        }
    }
    Ok(uri)
}

/// How many names are tried before giving up on finding a free one.
const NAME_TRIES: u32 = 100;

/// Take a screenshot: grab, compose, encode, and write a new file in the pictures folder. The
/// file is created, never overwritten. Returns the file's path.
pub fn take(setup: &ScreenshotSetup, seq: u32) -> Result<PathBuf, ShotError> {
    if !setup.pictures.is_absolute() {
        return Err(ShotError::RelativePath);
    }
    let png = encode_png(compose(&setup.grab.grab()?)?)?;
    std::fs::create_dir_all(&setup.pictures)?;
    for n in seq..seq.saturating_add(NAME_TRIES) {
        let path = setup
            .pictures
            .join(format!("Screenshot-{}-{n}.png", setup.stamp));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                file.write_all(&png)?;
                return Ok(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(std::io::Error::from(std::io::ErrorKind::AlreadyExists).into())
}

fn owned(value: impl Into<Value<'static>>) -> Option<OwnedValue> {
    OwnedValue::try_from(value.into()).ok()
}

/// The served interface.
#[derive(Debug)]
pub struct ScreenshotService {
    setup: Arc<ScreenshotSetup>,
    seq: AtomicU32,
}

impl Default for ScreenshotService {
    fn default() -> Self {
        Self::with_setup(ScreenshotSetup::unconfigured())
    }
}

impl ScreenshotService {
    /// A service with no screens: every screenshot is answered as failed.
    pub fn new() -> Self {
        Self::default()
    }

    /// A service taking screenshots as `setup` says.
    pub fn with_setup(setup: ScreenshotSetup) -> Self {
        Self {
            setup: Arc::new(setup),
            seq: AtomicU32::new(0),
        }
    }
}

/// The portal's response codes.
const RESPONSE_OK: u32 = 0;
const RESPONSE_CANCELLED: u32 = 1;
const RESPONSE_FAILED: u32 = 2;

#[zbus::interface(name = "org.freedesktop.impl.portal.Screenshot")]
impl ScreenshotService {
    /// Take a screenshot of every screen; the reply's `uri` names the PNG written to the
    /// person's pictures. The `interactive` and `modal` hints are not used: nothing is asked.
    async fn screenshot(
        &self,
        handle: ObjectPath<'_>,
        app_id: &str,
        parent_window: &str,
        options: Dict,
    ) -> (u32, Dict) {
        let _ = (handle, app_id, parent_window, options);
        let setup = Arc::clone(&self.setup);
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        let made = tokio::task::spawn_blocking(move || {
            let path = take(&setup, seq)?;
            file_uri(&path)
        })
        .await;
        match made {
            Ok(Ok(uri)) => {
                let mut results = Dict::new();
                if let Some(v) = owned(uri) {
                    results.insert("uri".to_owned(), v);
                }
                (RESPONSE_OK, results)
            }
            Ok(Err(error)) => {
                tracing::warn!(%error, "a screenshot failed");
                (RESPONSE_FAILED, Dict::new())
            }
            Err(error) => {
                tracing::warn!(%error, "the screenshot task did not finish");
                (RESPONSE_FAILED, Dict::new())
            }
        }
    }

    /// Let the person pick a colour from the screen; the reply's `color` is `(ddd)`. Until the
    /// shell offers a picker this is answered as cancelled.
    async fn pick_color(
        &self,
        handle: ObjectPath<'_>,
        app_id: &str,
        parent_window: &str,
        options: Dict,
    ) -> (u32, Dict) {
        let _ = (handle, app_id, parent_window, options);
        (RESPONSE_CANCELLED, Dict::new())
    }

    /// The screenshot targets offered: the whole screen.
    #[zbus(property)]
    fn available_targets(&self) -> u32 {
        1
    }

    /// The interface version.
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        3
    }
}

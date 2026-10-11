//! Where pixels come from.
//!
//! [`CaptureSource`] is the seam. [`ExtCapture`] is the real implementation, a client of the
//! Wayland image-capture protocols (`ext-image-capture-source` with `ext-image-copy-capture`),
//! so it works against any compositor that serves them; it picks the output or toplevel flavour.
//! [`FakeCapture`] serves recorded frames to tests.

mod fake;
pub mod output;
pub mod toplevel;

use std::os::fd::OwnedFd;

use transom_core::CastSource;

pub use fake::FakeCapture;

/// A size in physical pixels, as the capture produces it: for a window, logical size times
/// scale, rounded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PhysicalSizeWire {
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

/// A pixel layout, by the Wayland `wl_shm` names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PixelFormat {
    /// 32-bit with alpha.
    Argb8888,
    /// 32-bit, alpha ignored.
    Xrgb8888,
    /// 32-bit with alpha, byte order reversed.
    Abgr8888,
    /// 32-bit, byte order reversed, alpha ignored.
    Xbgr8888,
}

/// One plane of a GPU buffer.
#[derive(Debug)]
pub struct DmabufPlane {
    /// The buffer's file descriptor.
    pub fd: OwnedFd,
    /// Bytes from the start of the buffer.
    pub offset: u32,
    /// Bytes per row.
    pub stride: u32,
}

/// A GPU buffer handed through without copying.
#[derive(Debug)]
pub struct DmabufFrame {
    /// The DRM fourcc.
    pub fourcc: u32,
    /// The DRM format modifier.
    pub modifier: u64,
    /// The planes.
    pub planes: Vec<DmabufPlane>,
}

/// The pixels of a frame.
#[derive(Debug)]
#[non_exhaustive]
pub enum FrameData {
    /// Copied into memory, `stride` bytes per row.
    Shm {
        /// Layout of each pixel.
        format: PixelFormat,
        /// Bytes per row.
        stride: u32,
        /// The rows.
        bytes: Vec<u8>,
    },
    /// A GPU buffer.
    Dmabuf(DmabufFrame),
}

/// One captured frame.
#[derive(Debug)]
pub struct CapturedFrame {
    /// Its size.
    pub size: PhysicalSizeWire,
    /// Its pixels.
    pub data: FrameData,
    /// Presentation time in nanoseconds on the compositor's clock, as it reported it.
    pub presented_ns: u64,
}

/// Why a frame did not arrive.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CaptureError {
    /// The source is gone (the window closed, the display unplugged); no more frames will come.
    #[error("the source ended")]
    Ended,
    /// The compositor offers no way to capture this kind of source.
    #[error("the compositor cannot capture this source")]
    Unsupported,
    /// The compositor stopped the session.
    #[error("the compositor stopped the capture")]
    Stopped,
    /// The connection to the compositor failed.
    #[error("the display connection: {0}")]
    Connection(String),
}

/// A source of frames.
pub trait CaptureSource: Send + std::fmt::Debug {
    /// The next frame. Blocks until the source has something new (frames are damage-gated: a
    /// static picture yields none), and returns [`CaptureError::Ended`] once the source is gone.
    fn frame(&mut self) -> Result<CapturedFrame, CaptureError>;
    /// The size the next frame will have.
    fn size(&self) -> PhysicalSizeWire;
}

/// Which display socket to connect to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayName(pub String);

/// The real capture source: a client of the compositor's image-capture protocols.
#[derive(Debug)]
pub struct ExtCapture {
    inner: ExtKind,
}

#[derive(Debug)]
enum ExtKind {
    Output(output::OutputCapture),
    Toplevel(toplevel::ToplevelCapture),
}

impl ExtCapture {
    /// Connect to `display` and start capturing `source`.
    pub fn open(display: &DisplayName, source: &CastSource) -> Result<Self, CaptureError> {
        let inner = match source {
            CastSource::Output(name) => {
                ExtKind::Output(output::OutputCapture::open(display, name)?)
            }
            CastSource::Window(id) => {
                ExtKind::Toplevel(toplevel::ToplevelCapture::open(display, id)?)
            }
        };
        Ok(Self { inner })
    }
}

impl CaptureSource for ExtCapture {
    fn frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        match &mut self.inner {
            ExtKind::Output(c) => c.frame(),
            ExtKind::Toplevel(c) => c.frame(),
        }
    }

    fn size(&self) -> PhysicalSizeWire {
        match &self.inner {
            ExtKind::Output(c) => c.size(),
            ExtKind::Toplevel(c) => c.size(),
        }
    }
}

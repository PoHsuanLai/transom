//! Where pixels come from.
//!
//! [`CaptureSource`] is the seam. [`ExtCapture`] is the real implementation, a client of the
//! Wayland image-capture protocols (`ext-image-capture-source` with `ext-image-copy-capture`),
//! so it works against any compositor that serves them, for a display or for a window.
//! [`FakeCapture`] serves recorded frames to tests.

mod display;
mod fake;
mod job;
mod line;
mod pool;
mod wl;

use std::net::Shutdown;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::time::Duration;

use transom_core::{CastSource, CursorMode, Extent};

pub use display::{DisplayName, Route};
pub use fake::FakeCapture;
pub use job::Job;
pub use line::{CasementLine, FakeLine, LINE_DEADLINE, LineError, PortalLine};
pub use pool::{FramePool, POOL_KEEP, PixelBuffer};

use wl::Copier;

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
        /// The rows, in memory that goes back to its pool when the picture is dropped.
        bytes: PixelBuffer,
    },
    /// A GPU buffer.
    Dmabuf(DmabufFrame),
}

/// One captured frame.
#[derive(Debug)]
pub struct CapturedFrame {
    /// Its size.
    pub size: Extent,
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
    /// The compositor did not answer within the time allowed.
    #[error("the compositor did not answer in time")]
    TimedOut,
    /// The source is larger than a picture may be (an edge over 16384 pixels, or over 256 MiB).
    #[error("the source is too large to capture")]
    TooLarge,
}

/// What a wait for the next frame came to.
#[derive(Debug)]
#[non_exhaustive]
pub enum Arrival {
    /// A frame arrived.
    Frame(CapturedFrame),
    /// The wait ran out and the source is still there: nothing changed.
    Quiet,
}

/// A source of frames.
pub trait CaptureSource: Send + std::fmt::Debug {
    /// The next frame. Blocks until the source has something new (frames are damage-gated: a
    /// static picture yields none) but not for ever: a source that does not answer within its
    /// deadline is [`CaptureError::TimedOut`]. Returns [`CaptureError::Ended`] once the source is
    /// gone.
    fn frame(&mut self) -> Result<CapturedFrame, CaptureError>;
    /// As [`CaptureSource::frame`], but gives up after `wait` with [`Arrival::Quiet`]. A request
    /// for a frame stays open across calls, so no change is missed between them. A source that
    /// never blocks has nothing to wait for and just serves its frame.
    fn next_frame(&mut self, wait: Duration) -> Result<Arrival, CaptureError> {
        let _ = wait;
        self.frame().map(Arrival::Frame)
    }
    /// The size the next frame will have.
    fn size(&self) -> Extent;
    /// A handle that makes a [`CaptureSource::frame`] call that is blocked return an error, from
    /// any thread. `None` for a source that never blocks.
    fn canceller(&self) -> Option<Canceller> {
        None
    }
}

/// Unblocks a capture that is waiting for a frame by closing its connection.
#[derive(Debug)]
pub struct Canceller(UnixStream);

impl Canceller {
    /// A canceller for the connection behind `fd` (a duplicate of its socket).
    pub fn new(fd: OwnedFd) -> Self {
        Self(UnixStream::from(fd))
    }

    /// Close the connection. The capture's blocked call returns an error and later calls fail.
    pub fn cancel(&self) {
        let _ = self.0.shutdown(Shutdown::Both);
    }
}

/// The real capture source: a client of the compositor's image-capture protocols.
///
/// A display is captured through `ext_output_image_capture_source_manager_v1`; a window through
/// `ext_foreign_toplevel_image_capture_source_manager_v1`, using the identifier
/// `ext_foreign_toplevel_list_v1` publishes (frames are sized at logical size times scale and sent
/// only when the window changed). Both copy through `ext_image_copy_capture_manager_v1`.
#[derive(Debug)]
pub struct ExtCapture {
    copier: Copier,
}

impl ExtCapture {
    /// Start capturing `source` on the connection of `job`, with the pointer left out.
    pub fn open(job: &Job, source: &CastSource) -> Result<Self, CaptureError> {
        Self::open_with(job, source, CursorMode::Hidden)
    }

    /// As [`ExtCapture::open`], with the pointer drawn into the picture when `cursor` is
    /// [`CursorMode::Embedded`]. The metadata mode captures without the pointer. A window that is
    /// already gone is [`CaptureError::Ended`].
    pub fn open_with(
        job: &Job,
        source: &CastSource,
        cursor: CursorMode,
    ) -> Result<Self, CaptureError> {
        let mut link = job.link()?;
        let wl_source = match source {
            CastSource::Output(name) => link.output_source(name)?,
            CastSource::Window(id) => link.toplevel_source(id)?,
            // A kind of source a later transom-core adds and this capture does not know.
            _ => return Err(CaptureError::Unsupported),
        };
        Ok(Self {
            copier: Copier::start(link, wl_source, cursor)?,
        })
    }
}

impl CaptureSource for ExtCapture {
    fn frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        self.copier.frame()
    }

    fn next_frame(&mut self, wait: Duration) -> Result<Arrival, CaptureError> {
        Ok(match self.copier.frame_within(wait)? {
            Some(frame) => Arrival::Frame(frame),
            None => Arrival::Quiet,
        })
    }

    fn size(&self) -> Extent {
        self.copier.size()
    }

    fn canceller(&self) -> Option<Canceller> {
        self.copier.canceller()
    }
}

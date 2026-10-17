//! Where pixels come from.
//!
//! [`CaptureSource`] is the seam. [`ExtCapture`] is the real implementation, a client of the
//! Wayland image-capture protocols (`ext-image-capture-source` with `ext-image-copy-capture`),
//! so it works against any compositor that serves them; it picks the output or toplevel flavour.
//! [`FakeCapture`] serves recorded frames to tests.

mod display;
mod fake;
mod line;
pub mod output;
mod pool;
pub mod toplevel;
mod wl;

use std::net::Shutdown;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::time::Duration;

use transom_core::{CastSource, CursorMode, OutputFacts};

pub use display::{DisplayName, Route};
pub use fake::FakeCapture;
pub use line::{CasementLine, FakeLine, LINE_DEADLINE, LineError, PortalLine};
pub use pool::{FramePool, POOL_KEEP, PixelBuffer};

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
    fn size(&self) -> PhysicalSizeWire;
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

/// Every output and window that can be captured now, as the compositor lists them.
pub fn live_sources(display: &DisplayName) -> Result<Vec<CastSource>, CaptureError> {
    wl::Link::connect(display)?.live_sources()
}

/// Every output the compositor lists now: its name, place in the logical space, mode, turn and
/// scale. A fresh connection answers each call, so the list is current when it returns.
pub fn output_facts(display: &DisplayName) -> Result<Vec<OutputFacts>, CaptureError> {
    wl::Link::connect(display)?.output_facts()
}

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
    /// Connect to `display` and start capturing `source`, with the pointer left out.
    pub fn open(display: &DisplayName, source: &CastSource) -> Result<Self, CaptureError> {
        Self::open_with(display, source, CursorMode::Hidden)
    }

    /// As [`ExtCapture::open`], with the pointer drawn into the picture when `cursor` is
    /// [`CursorMode::Embedded`]. The metadata mode captures without the pointer.
    pub fn open_with(
        display: &DisplayName,
        source: &CastSource,
        cursor: CursorMode,
    ) -> Result<Self, CaptureError> {
        let inner = match source {
            CastSource::Output(name) => {
                ExtKind::Output(output::OutputCapture::open_with(display, name, cursor)?)
            }
            CastSource::Window(id) => {
                ExtKind::Toplevel(toplevel::ToplevelCapture::open_with(display, id, cursor)?)
            }
            // A kind of source a later transom-core adds and this capture does not know.
            _ => return Err(CaptureError::Unsupported),
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

    fn next_frame(&mut self, wait: Duration) -> Result<Arrival, CaptureError> {
        match &mut self.inner {
            ExtKind::Output(c) => c.next_frame(wait),
            ExtKind::Toplevel(c) => c.next_frame(wait),
        }
    }

    fn size(&self) -> PhysicalSizeWire {
        match &self.inner {
            ExtKind::Output(c) => c.size(),
            ExtKind::Toplevel(c) => c.size(),
        }
    }

    fn canceller(&self) -> Option<Canceller> {
        match &self.inner {
            ExtKind::Output(c) => c.canceller(),
            ExtKind::Toplevel(c) => c.canceller(),
        }
    }
}

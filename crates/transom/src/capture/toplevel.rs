//! Capturing one window: `ext_foreign_toplevel_image_capture_source_manager_v1` with
//! `ext_image_copy_capture_manager_v1`, using the identifier `ext_foreign_toplevel_list_v1`
//! publishes. The compositor sizes frames at logical size times scale and sends one only when the
//! window changed.

use std::time::Duration;

use transom_core::CursorMode;

use super::wl::Copier;
use super::{
    Arrival, Canceller, CaptureError, CaptureSource, CapturedFrame, Job, PhysicalSizeWire,
};

/// A running capture of one window.
#[derive(Debug)]
pub struct ToplevelCapture {
    copier: Copier,
}

impl ToplevelCapture {
    /// On the connection of `job`, find the toplevel with this identifier, create its capture
    /// source and session. A window that is already gone is [`CaptureError::Ended`]. The pointer
    /// is left out.
    pub fn open(job: &Job, identifier: &str) -> Result<Self, CaptureError> {
        Self::open_with(job, identifier, CursorMode::Hidden)
    }

    /// As [`ToplevelCapture::open`], with the pointer drawn into the picture when `cursor` is
    /// [`CursorMode::Embedded`].
    pub fn open_with(
        job: &Job,
        identifier: &str,
        cursor: CursorMode,
    ) -> Result<Self, CaptureError> {
        let mut link = job.link()?;
        let source = link.toplevel_source(identifier)?;
        Ok(Self {
            copier: Copier::start(link, source, cursor)?,
        })
    }
}

impl CaptureSource for ToplevelCapture {
    /// The next frame. Blocks until the window changes; [`CaptureError::Ended`] once it closes.
    fn frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        self.copier.frame()
    }

    fn next_frame(&mut self, wait: Duration) -> Result<Arrival, CaptureError> {
        Ok(match self.copier.frame_within(wait)? {
            Some(frame) => Arrival::Frame(frame),
            None => Arrival::Quiet,
        })
    }

    fn size(&self) -> PhysicalSizeWire {
        self.copier.size()
    }

    fn canceller(&self) -> Option<Canceller> {
        self.copier.canceller()
    }
}

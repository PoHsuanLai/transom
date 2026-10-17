//! Capturing a whole display: `ext_output_image_capture_source_manager_v1` with
//! `ext_image_copy_capture_manager_v1`.

use std::time::Duration;

use transom_core::CursorMode;

use super::wl::Copier;
use super::{
    Arrival, Canceller, CaptureError, CaptureSource, CapturedFrame, Job, PhysicalSizeWire,
};

/// A running capture of one output.
#[derive(Debug)]
pub struct OutputCapture {
    copier: Copier,
}

impl OutputCapture {
    /// On the connection of `job`, find the output called `name`, create its capture source and
    /// session, and wait for the compositor's buffer constraints. The pointer is left out of the
    /// picture.
    pub fn open(job: &Job, name: &str) -> Result<Self, CaptureError> {
        Self::open_with(job, name, CursorMode::Hidden)
    }

    /// As [`OutputCapture::open`], with the pointer drawn into the picture when `cursor` is
    /// [`CursorMode::Embedded`].
    pub fn open_with(job: &Job, name: &str, cursor: CursorMode) -> Result<Self, CaptureError> {
        let mut link = job.link()?;
        let source = link.output_source(name)?;
        Ok(Self {
            copier: Copier::start(link, source, cursor)?,
        })
    }
}

impl CaptureSource for OutputCapture {
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

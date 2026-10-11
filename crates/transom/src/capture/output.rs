//! Capturing a whole display: `ext_output_image_capture_source_manager_v1` with
//! `ext_image_copy_capture_manager_v1`.

use super::{CaptureError, CaptureSource, CapturedFrame, DisplayName, PhysicalSizeWire};

/// A running capture of one output.
#[derive(Debug)]
pub struct OutputCapture {
    size: PhysicalSizeWire,
}

impl OutputCapture {
    /// Connect, find the output called `name`, create its capture source and session, and wait
    /// for the compositor's buffer constraints (P2-T2).
    pub fn open(display: &DisplayName, name: &str) -> Result<Self, CaptureError> {
        let _ = (display, name);
        todo!("P2-T2: ext-image-copy-capture session for an output")
    }
}

impl CaptureSource for OutputCapture {
    fn frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        let _ = &self.size;
        todo!("P2-T2: attach a buffer, capture, wait for ready or failed, convert to CapturedFrame")
    }

    fn size(&self) -> PhysicalSizeWire {
        self.size
    }
}

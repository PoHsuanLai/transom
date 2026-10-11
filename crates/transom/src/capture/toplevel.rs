//! Capturing one window: `ext_foreign_toplevel_image_capture_source_manager_v1` with
//! `ext_image_copy_capture_manager_v1`, using the identifier `ext_foreign_toplevel_list_v1`
//! publishes. The compositor sizes frames at logical size times scale and sends one only when the
//! window changed.

use super::{CaptureError, CaptureSource, CapturedFrame, DisplayName, PhysicalSizeWire};

/// A running capture of one window.
#[derive(Debug)]
pub struct ToplevelCapture {
    size: PhysicalSizeWire,
}

impl ToplevelCapture {
    /// Connect, find the toplevel with this identifier, create its capture source and session
    /// (P2-T2). A window that is already gone is [`CaptureError::Ended`].
    pub fn open(display: &DisplayName, identifier: &str) -> Result<Self, CaptureError> {
        let _ = (display, identifier);
        todo!("P2-T2: ext-image-copy-capture session for a foreign toplevel")
    }
}

impl CaptureSource for ToplevelCapture {
    fn frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        let _ = &self.size;
        todo!(
            "P2-T2: wait for damage, capture, convert to CapturedFrame; Ended when the toplevel closes"
        )
    }

    fn size(&self) -> PhysicalSizeWire {
        self.size
    }
}

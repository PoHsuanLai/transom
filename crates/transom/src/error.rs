//! The errors the service reports. Library paths return these; nothing panics on a bad request.

use transom_core::ChoiceRefusal;

/// Why the portal could not start or finish something.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TransomError {
    /// The bus refused us (no session bus, the name is taken, a call failed).
    #[error("the session bus: {0}")]
    Bus(#[from] zbus::Error),
    /// A capture source failed.
    #[error("capture: {0}")]
    Capture(#[from] crate::capture::CaptureError),
    /// The chooser could not be asked, or its answer was refused.
    #[error("chooser: {0}")]
    Chooser(#[from] crate::chooser::ChooserError),
    /// The stream sink failed.
    #[error("stream: {0}")]
    Stream(#[from] crate::screencast::stream::SinkError),
    /// A restore store failed.
    #[error("restore store: {0}")]
    Restore(#[from] crate::screencast::restore::StoreError),
    /// The appearance could not be read.
    #[error("settings: {0}")]
    Settings(#[from] crate::settings::AppearanceError),
    /// An answer was not acceptable for the request.
    #[error("choice: {0}")]
    Choice(#[from] ChoiceRefusal),
}

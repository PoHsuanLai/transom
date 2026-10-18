//! The errors the service reports. Library paths return these; nothing panics on a bad request.

/// Why the portal could not start or finish something.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum TransomError {
    /// The bus refused us (no session bus, the name is taken, a call failed).
    #[error("the session bus: {0}")]
    Bus(#[from] zbus::Error),
}

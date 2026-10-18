//! The portal's own connection to the compositor.
//!
//! A program on the compositor's public socket can be shown to be no more than a program, so its
//! first screenshot or share would raise the compositor's own question on top of the portal's
//! chooser. casement hands the portal a connection of its own instead, over the session bus
//! (`org.quire.Compositor1.Connect("portal")`), where the portal is proven all the way; that
//! connection is trusted for the screen and the windows and for nothing else. [`PortalLine`] is
//! the seam that asks for it, [`CasementLine`] the real one and [`FakeLine`] a test's. When no such
//! connection can be had (another compositor, an older casement) the capture connects the way any
//! Wayland client does, and the portal's own sheet is the only question ([`super::DisplayName`]).
//! The compositor limits how often one caller may ask, so a capture job asks once and runs all its
//! steps on the connection it gets ([`super::Job`]).

mod casement;
mod fake;

use std::fmt;
use std::os::fd::OwnedFd;
use std::time::Duration;

pub use self::casement::CasementLine;
pub use self::fake::FakeLine;

/// How long the compositor has to hand out the connection before the attempt is dropped: it is a
/// local call that answers at once, so a longer wait means it is stuck.
pub const LINE_DEADLINE: Duration = Duration::from_secs(10);

/// Why the compositor did not hand out a connection.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LineError {
    /// Nothing on the bus hands out connections (another compositor, or none running).
    #[error("no compositor on the bus hands out connections")]
    Unavailable,
    /// The compositor was answering other calls and asked to be tried again in a moment.
    #[error("the compositor was busy with other calls")]
    Busy,
    /// The compositor refused, or the call failed.
    #[error("the compositor did not hand out a connection: {0}")]
    Refused(String),
    /// The compositor did not answer in time.
    #[error("the compositor did not answer in time")]
    TimedOut,
}

/// The seam: a Wayland connection of the portal's own.
pub trait PortalLine: Send + Sync + fmt::Debug {
    /// The client end of a new connection, trusted for what the portal is. Blocks until the
    /// compositor answers, for at most [`LINE_DEADLINE`].
    fn open(&self) -> Result<OwnedFd, LineError>;
}

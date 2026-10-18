//! Which display a capture connects to, and how.

use std::os::fd::OwnedFd;
use std::sync::{Arc, OnceLock};

use super::line::{LineError, PortalLine};

/// How a connection to the display is made.
#[derive(Debug)]
#[non_exhaustive]
pub enum Route {
    /// A connection the compositor handed this portal over the bus, trusted for what the portal is.
    Handed(OwnedFd),
    /// The session's own display, found the way Wayland clients find theirs.
    Env,
    /// The socket at this path, as given (it must be absolute).
    Path(String),
}

/// A line to the compositor, and whether the log has already said it was not used.
#[derive(Clone, Debug)]
struct Handout {
    line: Arc<dyn PortalLine>,
    said: Arc<OnceLock<()>>,
}

impl Handout {
    /// A connection from the compositor, or the session's own display when it hands none out
    /// (the log says so, once).
    fn route(&self) -> Route {
        match self.line.open() {
            Ok(fd) => Route::Handed(fd),
            Err(error) => {
                self.say_once(&error);
                Route::Env
            }
        }
    }

    fn say_once(&self, error: &LineError) {
        if self.said.set(()).is_ok() {
            tracing::warn!(
                %error,
                "the compositor handed this portal no connection of its own (another compositor, or an older one); capturing on the session's display, where its own question may be asked"
            );
        }
    }
}

/// Which display socket to connect to: the session's own, else an absolute socket path. The
/// session's own is first asked of the compositor as a connection handed to this portal
/// ([`DisplayName::with_portal_line`]).
#[derive(Clone, Debug)]
pub struct DisplayName(Target);

/// What a [`DisplayName`] names.
#[derive(Clone, Debug)]
enum Target {
    /// The session's own display, with the line to ask first, if any.
    Session { handout: Option<Handout> },
    /// The socket at this path.
    At(String),
}

impl DisplayName {
    /// The session's own display, found the way Wayland clients find theirs.
    pub fn session() -> Self {
        Self(Target::Session { handout: None })
    }

    /// The display socket at `path`, which must be absolute. A compositor is never asked for a
    /// connection of the portal's own.
    pub fn at(path: impl Into<String>) -> Self {
        Self(Target::At(path.into()))
    }

    /// This display, with `line` asked first for a connection of the portal's own when the display
    /// is the session's. Where it hands none out, the session's display is used as before.
    pub fn with_portal_line(self, line: Arc<dyn PortalLine>) -> Self {
        match self.0 {
            Target::Session { .. } => Self(Target::Session {
                handout: Some(Handout {
                    line,
                    said: Arc::new(OnceLock::new()),
                }),
            }),
            at @ Target::At(_) => Self(at),
        }
    }

    /// How to connect now. Asks the line (if there is one) afresh on every call, so this blocks
    /// until the compositor answers or the deadline passes; it is a call per connection, and the
    /// compositor limits how often one caller may make it, so a capture job calls it once
    /// ([`Job`](super::Job)) and runs all its steps on what it gets.
    pub fn route(&self) -> Route {
        match &self.0 {
            Target::Session {
                handout: Some(handout),
            } => handout.route(),
            Target::Session { handout: None } => Route::Env,
            Target::At(path) => Route::Path(path.clone()),
        }
    }
}

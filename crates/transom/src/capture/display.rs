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
pub struct DisplayName {
    name: String,
    handout: Option<Handout>,
}

impl DisplayName {
    /// The session's own display, found the way Wayland clients find theirs.
    pub fn session() -> Self {
        Self {
            name: String::new(),
            handout: None,
        }
    }

    /// The display socket at `path`, which must be absolute. A compositor is never asked for a
    /// connection of the portal's own.
    pub fn at(path: impl Into<String>) -> Self {
        Self {
            name: path.into(),
            handout: None,
        }
    }

    /// This display, with `line` asked first for a connection of the portal's own when the display
    /// is the session's. Where it hands none out, the session's display is used as before.
    pub fn with_portal_line(self, line: Arc<dyn PortalLine>) -> Self {
        Self {
            handout: Some(Handout {
                line,
                said: Arc::new(OnceLock::new()),
            }),
            ..self
        }
    }

    /// How to connect now. Asks the line (if there is one) for each connection of the session's
    /// display, so this blocks until the compositor answers or the deadline passes.
    pub fn route(&self) -> Route {
        match (self.name.as_str(), &self.handout) {
            ("", Some(handout)) => handout.route(),
            ("", None) => Route::Env,
            (path, _) => Route::Path(path.to_owned()),
        }
    }
}

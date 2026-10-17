//! The one connection of a capture job, and the way to take sessions on it.

use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use wayland_client::Connection;
use wayland_client::globals::{GlobalList, registry_queue_init};

use super::wait::Watchdog;
use super::{Link, Wl, broken};
use crate::capture::{CaptureError, DisplayName, Route};

/// How long the compositor has to accept the connection and list its globals. It does both at
/// once; a longer wait means it is stalled (a GPU reset, a frozen session).
const CONNECT_DEADLINE: Duration = Duration::from_secs(5);

/// A connection to the compositor and the globals it listed on it. A capture job opens it once and
/// every step of the job runs on it, each through a [`Link`] of its own, so the compositor is
/// asked for a connection once however many outputs or windows the job touches. It closes when the
/// job and every capture running on it are gone.
#[derive(Debug)]
pub(in crate::capture) struct Wire {
    pub(super) conn: Connection,
    pub(super) globals: GlobalList,
}

impl Wire {
    /// Connect to the display: a connection the compositor hands this portal, else the session's
    /// own when `display` is the session's, else the socket at the absolute path it holds. The
    /// choice is made here, once, and holds for everything on the connection.
    pub fn open(display: &DisplayName) -> Result<Arc<Self>, CaptureError> {
        let conn = match display.route() {
            Route::Handed(fd) => Connection::from_socket(UnixStream::from(fd)).map_err(broken)?,
            Route::Env => Connection::connect_to_env().map_err(broken)?,
            Route::Path(path) if Path::new(&path).is_absolute() => {
                let stream = UnixStream::connect(&path).map_err(broken)?;
                Connection::from_socket(stream).map_err(broken)?
            }
            _ => {
                return Err(CaptureError::Connection(
                    "a display name must be a socket path".to_owned(),
                ));
            }
        };
        // Listing the globals is a round trip with no deadline of its own: a watchdog cuts the
        // connection if it takes too long, so a stalled compositor fails the call.
        let watchdog = Watchdog::arm(&conn, CONNECT_DEADLINE);
        let listed = registry_queue_init::<Wl>(&conn);
        drop(watchdog);
        // The queue that came with the list is not kept: the list keeps itself current as events
        // are read, and each session brings a queue of its own.
        let (globals, _first) = listed.map_err(broken)?;
        Ok(Arc::new(Self { conn, globals }))
    }

    /// A session on this connection, with an event queue and state of its own. Asks the
    /// compositor for nothing.
    pub fn link(self: &Arc<Self>) -> Link {
        let queue = self.conn.new_event_queue();
        let qh = queue.handle();
        Link {
            wire: Arc::clone(self),
            queue,
            qh,
            wl: Wl::default(),
        }
    }
}

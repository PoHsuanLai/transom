//! The real line: `org.quire.Compositor1.Connect("portal")` over the session bus.

use std::os::fd::OwnedFd;
use std::sync::mpsc;
use std::time::Duration;

use super::{LINE_DEADLINE, LineError, PortalLine};
use crate::link::BusLink;
use crate::remote_desktop::eis::Compositor1Proxy;

/// The role the compositor knows this portal's connection by.
const ROLE: &str = "portal";

/// How long to wait before asking again after the compositor asked to be tried again later. It
/// counts the calls of one caller in a one-second window. A capture job asks once, so this is for
/// several jobs (screenshots, share starts) arriving together.
const BUSY_PAUSE: Duration = Duration::from_millis(1100);

/// Asks through the connection that owns the portal's name (the compositor tells its caller by
/// that connection, and proves it all the way), the same one the input sockets come through.
#[derive(Debug)]
pub struct CasementLine {
    link: BusLink,
}

impl CasementLine {
    /// A line that asks through `link`.
    pub fn new(link: BusLink) -> Self {
        Self { link }
    }
}

impl PortalLine for CasementLine {
    /// Runs the call on the runtime that serves the bus and waits for its answer here, so it may
    /// be asked from a blocking thread (where captures connect) without starting a runtime.
    fn open(&self) -> Result<OwnedFd, LineError> {
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| LineError::Refused("no bus runtime on this thread".to_owned()))?;
        let (answer, wait) = mpsc::channel();
        let link = self.link.clone();
        runtime.spawn(async move {
            let asked = tokio::time::timeout(LINE_DEADLINE, ask(&link)).await;
            let _ = answer.send(asked.unwrap_or(Err(LineError::TimedOut)));
        });
        wait.recv()
            .unwrap_or_else(|_| Err(LineError::Refused("the bus runtime stopped".to_owned())))
    }
}

/// Asks once, and once more after a pause when the compositor said it was busy.
async fn ask(link: &BusLink) -> Result<OwnedFd, LineError> {
    match ask_once(link).await {
        Err(LineError::Busy) => {
            tokio::time::sleep(BUSY_PAUSE).await;
            ask_once(link).await
        }
        other => other,
    }
}

async fn ask_once(link: &BusLink) -> Result<OwnedFd, LineError> {
    let connection = link.connection().map_err(refused)?;
    let proxy = Compositor1Proxy::new(&connection).await.map_err(refused)?;
    proxy
        .connect(ROLE)
        .await
        .map(OwnedFd::from)
        .map_err(|error| classify(&error))
}

fn refused(error: impl std::fmt::Display) -> LineError {
    LineError::Refused(error.to_string())
}

/// What a failed call means for the capture.
fn classify(error: &zbus::Error) -> LineError {
    match error {
        zbus::Error::MethodError(name, ..) if ends_with_any(name.as_str(), &NOBODY) => {
            LineError::Unavailable
        }
        zbus::Error::MethodError(name, ..) if ends_with_any(name.as_str(), &BUSY) => {
            LineError::Busy
        }
        other => refused(other),
    }
}

/// The errors the bus gives for a name nobody owns.
const NOBODY: [&str; 2] = ["ServiceUnknown", "NameHasNoOwner"];

/// The error casement gives a caller over its rate.
const BUSY: [&str; 1] = ["LimitsExceeded"];

fn ends_with_any(name: &str, tails: &[&str]) -> bool {
    tails.iter().any(|tail| name.ends_with(tail))
}

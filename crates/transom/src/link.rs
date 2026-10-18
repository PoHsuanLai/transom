//! The one bus connection that owns the portal's name.
//!
//! Every question this portal asks (the picker, a yes-or-no sheet, the colour picker, the input
//! socket) must come from the connection that owns [`crate::BUS_NAME`]: the shell answers only
//! the owner of that name, and a second connection has a different unique name, so the shell would
//! see a stranger. [`BusLink`] is the handle the asking seams share. It is empty until [`crate::serve`]
//! has built the connection, then holds it for the life of the process. The same handle carries
//! [`crate::peers::Peers`], the means of telling which program is on the other end of a call: it is
//! built on that connection too, so it is attached with it.

use std::sync::{Arc, OnceLock};

use crate::chooser::ShellError;
use crate::peers::Peers;

/// A shared, set-once handle to the name-owning connection.
#[derive(Clone, Debug, Default)]
pub struct BusLink {
    connection: Arc<OnceLock<zbus::Connection>>,
    peers: Arc<OnceLock<Arc<Peers>>>,
}

impl BusLink {
    /// A link nothing is attached to yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Attach the connection that owns the portal's name. Only the first is kept.
    pub fn attach(&self, connection: zbus::Connection) {
        let _ = self.connection.set(connection);
    }

    /// The name-owning connection, or why there is none yet.
    pub fn connection(&self) -> Result<zbus::Connection, ShellError> {
        self.connection
            .get()
            .cloned()
            .ok_or_else(|| ShellError::Call("the portal is not on the bus yet".to_owned()))
    }

    /// Attach the means of identifying callers. Only the first is kept.
    pub fn identify_with(&self, peers: Peers) {
        let _ = self.peers.set(Arc::new(peers));
    }

    /// The means of identifying callers, or `None` before [`crate::serve`] has attached them. A
    /// caller that finds none refuses: nobody can be told apart yet.
    pub fn peers(&self) -> Option<Arc<Peers>> {
        self.peers.get().cloned()
    }
}

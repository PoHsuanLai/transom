//! The one bus connection that owns the portal's name.
//!
//! Every question this portal asks (the picker, a yes-or-no sheet, the colour picker, the input
//! socket) must come from the connection that owns [`crate::BUS_NAME`]: the shell answers only
//! the owner of that name, and a second connection has a different unique name, so the shell would
//! see a stranger. [`BusLink`] is the handle the asking seams share. It is empty until [`crate::serve`]
//! has built the connection, then holds it for the life of the process.

use std::sync::{Arc, OnceLock};

use crate::chooser::ChooserError;

/// A shared, set-once handle to the name-owning connection.
#[derive(Clone, Debug, Default)]
pub struct BusLink {
    connection: Arc<OnceLock<zbus::Connection>>,
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
    pub fn connection(&self) -> Result<zbus::Connection, ChooserError> {
        self.connection
            .get()
            .cloned()
            .ok_or_else(|| ChooserError::Call("the portal is not on the bus yet".to_owned()))
    }
}

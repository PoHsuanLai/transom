//! `org.freedesktop.impl.portal.ScreenCast`: sharing a display or a window as a PipeWire stream.
//!
//! The call sequence is the portal's: `CreateSession`, `SelectSources`, `Start`, then the app
//! reads the node the reply names. The decisions are `transom_core::step` on a [`session::SessionBook`];
//! this interface moves the wire values in and out and drives the seams.

mod flow;
pub mod options;
pub mod pipewire;
pub use self::pipewire::PipewireSink;
pub mod pixels;
pub mod restore;
pub mod run;
pub mod session;
pub mod stream;

use std::collections::HashMap;
use std::sync::Arc;

use zbus::zvariant::{ObjectPath, OwnedValue};

use crate::chooser::Chooser;
use crate::peers::Tether;
use flow::Shared;
use restore::RestoreStore;
use session::SessionKey;

/// The wire shape of an options or results dictionary.
pub type Dict = HashMap<String, OwnedValue>;

/// The served interface.
#[derive(Debug)]
pub struct ScreenCastService {
    shared: Arc<Shared>,
}

impl ScreenCastService {
    /// A service with no sessions, asking `chooser` and keeping tokens in `restore`.
    pub fn new(chooser: Box<dyn Chooser>, restore: Box<dyn RestoreStore>) -> Self {
        Self {
            shared: Arc::new(Shared::new(chooser, restore)),
        }
    }

    /// End every session hung on `peer` (a unique name that left the bus, or stopped being the
    /// portal frontend): its streams stop and its session objects leave the bus.
    pub async fn peer_gone(&self, peer: &str, connection: &zbus::Connection) {
        for key in self.shared.close_for_peer(peer) {
            flow::forget_object(connection, &key).await;
        }
    }

    /// This service showing apps by the names `names` finds. Call it before the service is shared.
    pub fn with_names(mut self, names: crate::names::AppNames) -> Self {
        if let Some(shared) = Arc::get_mut(&mut self.shared) {
            shared.names = names;
        }
        self
    }
}

/// The object the portal frontend closes a session through, served at the session's path:
/// `org.freedesktop.impl.portal.Session`.
#[derive(Debug)]
pub struct SessionObject {
    key: SessionKey,
    shared: Arc<Shared>,
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Session")]
impl SessionObject {
    /// The app, or the portal, ended the session: stop its streams.
    async fn close(&self, #[zbus(connection)] connection: &zbus::Connection) {
        self.shared.close(&self.key);
        flow::forget_object(connection, &self.key).await;
    }
}

#[zbus::interface(name = "org.freedesktop.impl.portal.ScreenCast")]
impl ScreenCastService {
    /// Open a session. `options` is empty today; the reply carries `session_id`.
    async fn create_session(
        &self,
        handle: ObjectPath<'_>,
        session_handle: ObjectPath<'_>,
        app_id: &str,
        options: Dict,
        #[zbus(object_server)] server: &zbus::ObjectServer,
        #[zbus(header)] header: zbus::message::Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> (u32, Dict) {
        let _ = (handle, options);
        let key = SessionKey(session_handle.to_string());
        let tether = Tether::of(connection, &header).await;
        match self.shared.create(&key, app_id, tether) {
            Ok(dropped) => {
                for old in dropped {
                    flow::forget_object(connection, &old).await;
                }
            }
            Err(_) => return (2, Dict::new()),
        }
        let object = SessionObject {
            key: key.clone(),
            shared: Arc::clone(&self.shared),
        };
        if server.at(session_handle.as_str(), object).await.is_err() {
            self.shared.close(&key);
            return (2, Dict::new());
        }
        let mut results = Dict::new();
        if let Ok(id) = OwnedValue::try_from(zbus::zvariant::Value::new(key.0)) {
            results.insert("session_id".to_owned(), id);
        }
        (0, results)
    }

    /// Record `types`, `multiple`, `cursor_mode`, `persist_mode` and `restore_data`.
    async fn select_sources(
        &self,
        handle: ObjectPath<'_>,
        session_handle: ObjectPath<'_>,
        app_id: &str,
        options: Dict,
    ) -> (u32, Dict) {
        let _ = handle;
        let key = SessionKey(session_handle.to_string());
        let selection = options::selection(&options);
        match self.shared.select(&key, app_id, selection) {
            true => (0, Dict::new()),
            false => (2, Dict::new()),
        }
    }

    /// Ask the person if the token did not resolve, open the stream, reply with `streams` and,
    /// when persisting, `restore_data`.
    async fn start(
        &self,
        handle: ObjectPath<'_>,
        session_handle: ObjectPath<'_>,
        app_id: &str,
        parent_window: &str,
        options: Dict,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> (u32, Dict) {
        let _ = (handle, parent_window, options);
        let key = SessionKey(session_handle.to_string());
        self.shared
            .start(&key, app_id, connection)
            .await
            .into_wire()
    }

    /// The source types served: displays and windows (1 and 2).
    #[zbus(property)]
    fn available_source_types(&self) -> u32 {
        3
    }

    /// The cursor modes served: hidden, embedded, metadata (1, 2 and 4).
    #[zbus(property)]
    fn available_cursor_modes(&self) -> u32 {
        7
    }

    /// The interface version.
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        5
    }
}

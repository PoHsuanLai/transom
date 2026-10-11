//! `org.freedesktop.impl.portal.ScreenCast`: sharing a display or a window as a PipeWire stream.
//!
//! The call sequence is the portal's: `CreateSession`, `SelectSources`, `Start`, then the app
//! reads the node the reply names. The decisions are `transom_core::step` on a [`session::SessionBook`];
//! this interface moves the wire values in and out and drives the seams.

pub mod pipewire;
pub use self::pipewire::PipewireSink;
pub mod restore;
pub mod session;
pub mod stream;

use std::collections::HashMap;
use std::sync::Mutex;

use zbus::zvariant::{ObjectPath, OwnedValue};

use crate::chooser::Chooser;
use restore::RestoreStore;
use session::SessionBook;

/// The wire shape of an options or results dictionary.
pub type Dict = HashMap<String, OwnedValue>;

/// The served interface.
#[derive(Debug)]
pub struct ScreenCastService {
    #[expect(dead_code, reason = "P2-T2")]
    sessions: Mutex<SessionBook>,
    #[expect(dead_code, reason = "P2-T2")]
    chooser: Box<dyn Chooser>,
    #[expect(dead_code, reason = "P2-T2")]
    restore: Box<dyn RestoreStore>,
}

impl ScreenCastService {
    /// A service with no sessions, asking `chooser` and keeping tokens in `restore`.
    pub fn new(chooser: Box<dyn Chooser>, restore: Box<dyn RestoreStore>) -> Self {
        Self {
            sessions: Mutex::new(SessionBook::new()),
            chooser,
            restore,
        }
    }
}

#[zbus::interface(name = "org.freedesktop.impl.portal.ScreenCast")]
impl ScreenCastService {
    /// Open a session. `options` is empty today.
    async fn create_session(
        &self,
        handle: ObjectPath<'_>,
        session_handle: ObjectPath<'_>,
        app_id: &str,
        options: Dict,
    ) -> (u32, Dict) {
        let _ = (handle, session_handle, app_id, options);
        todo!("P2-T2: SessionBook::create; reply 0 and an empty dict")
    }

    /// Record `types`, `multiple`, `cursor_mode`, `persist_mode` and `restore_data`.
    async fn select_sources(
        &self,
        handle: ObjectPath<'_>,
        session_handle: ObjectPath<'_>,
        app_id: &str,
        options: Dict,
    ) -> (u32, Dict) {
        let _ = (handle, session_handle, app_id, options);
        todo!("P2-T2: parse the options into a Selection; SessionBook::select")
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
    ) -> (u32, Dict) {
        let _ = (handle, session_handle, app_id, parent_window, options);
        todo!(
            "P2-T2: CastIn::Start, run the CastOuts: chooser, ExtCapture, PipewireSink, Pump thread"
        )
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

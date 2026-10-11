//! `org.freedesktop.impl.portal.Inhibit`: keeping the screen on, or the session from ending.
//!
//! The decisions are `transom_core::InhibitBook`; the effect (telling the session) is the
//! P2-T1 fill.

use std::sync::Mutex;

use transom_core::InhibitBook;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::ObjectPath;

use crate::screencast::Dict;

/// The served interface.
#[derive(Debug, Default)]
pub struct InhibitService {
    #[expect(dead_code, reason = "P2-T1")]
    book: Mutex<InhibitBook>,
}

impl InhibitService {
    /// A service holding no requests.
    pub fn new() -> Self {
        Self::default()
    }
}

// The D-Bus interface lives in a private module: zbus's macro generates undocumented
// property-change methods, which the crate's `missing_docs` would flag on a public impl.
mod interface {
    use super::*;

    #[zbus::interface(name = "org.freedesktop.impl.portal.Inhibit")]
    impl InhibitService {
        /// Hold a request until its request object is closed.
        async fn inhibit(
            &self,
            handle: ObjectPath<'_>,
            app_id: &str,
            window: &str,
            flags: u32,
            options: Dict,
        ) {
            let _ = (handle, app_id, window, flags, options);
            todo!("P2-T1: InhibitBook::hold, release when the request object goes away")
        }

        /// Start watching the session state (screen locked, idle); returns the response code.
        async fn create_monitor(
            &self,
            handle: ObjectPath<'_>,
            session_handle: ObjectPath<'_>,
            app_id: &str,
            window: &str,
        ) -> u32 {
            let _ = (handle, session_handle, app_id, window);
            todo!("P2-T1: register the monitor session")
        }

        /// An app answered the end-of-session query.
        async fn query_end_response(&self, session_handle: ObjectPath<'_>) {
            let _ = session_handle;
            todo!("P2-T1: note the answer")
        }

        /// The session state changed (`screensaver-active`, `session-state`).
        #[zbus(signal)]
        async fn state_changed(
            emitter: &SignalEmitter<'_>,
            session_handle: ObjectPath<'_>,
            state: Dict,
        ) -> zbus::Result<()>;

        /// The interface version.
        #[zbus(property, name = "version")]
        fn version(&self) -> u32 {
            3
        }
    }
}

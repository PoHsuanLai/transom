//! `org.freedesktop.impl.portal.Inhibit`: keeping the screen on, or the session from ending.
//!
//! The decisions are `transom_core::InhibitBook`. [`InhibitHub`] holds the book together with the
//! effect (telling the session what is now held, behind [`InhibitEffect`]) and the monitors, so
//! the whole of it runs in tests against [`FakeEffect`]. The service object is a thin wrapper
//! that exports the portal's request and session objects. A hold hangs on the peer that asked
//! and on the portal frontend: when either leaves the bus the hold is released.

mod effect;
mod hub;

use std::sync::Arc;

use tokio::sync::Mutex as AsyncMutex;
use transom_core::{InhibitFlags, RequestHandle};
use zbus::ObjectServer;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

pub use self::effect::{
    EffectError, FakeEffect, InhibitEffect, LOGIND_DEADLINE, LogindInhibitor, logind_what,
};
pub use self::hub::{
    HoldError, InhibitHub, MAX_MONITORS, MonitorAdded, MonitorKnown, MonitorState,
};
use crate::peers::Tether;
use crate::screencast::Dict;

type SharedHub = Arc<AsyncMutex<InhibitHub>>;

/// The portal's request object for one held inhibition: closing it releases the hold.
#[derive(Debug)]
struct RequestObject {
    hub: SharedHub,
    key: RequestHandle,
    path: OwnedObjectPath,
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Request")]
impl RequestObject {
    /// The caller let go: release the hold and take the object off the bus.
    async fn close(&self, #[zbus(object_server)] server: &ObjectServer) {
        if let Err(error) = self.hub.lock().await.release(&self.key).await {
            tracing::warn!(%error, "a released hold could not be passed on");
        }
        let _ = server.remove::<RequestObject, _>(self.path.as_str()).await;
    }
}

/// The portal's session object for one monitor: closing it stops the monitor.
#[derive(Debug)]
struct MonitorObject {
    hub: SharedHub,
    session: String,
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Session")]
impl MonitorObject {
    /// The caller let go: stop monitoring and take the object off the bus.
    async fn close(&self, #[zbus(object_server)] server: &ObjectServer) {
        self.hub.lock().await.remove_monitor(&self.session);
        let _ = server
            .remove::<MonitorObject, _>(self.session.as_str())
            .await;
    }

    /// The interface version.
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        1
    }
}

/// The session state a monitor is first told: running, screensaver off.
fn initial_state() -> Dict {
    let mut state = Dict::new();
    for (key, value) in [
        ("screensaver-active", Value::Bool(false)),
        ("session-state", Value::U32(1)),
    ] {
        if let Ok(v) = OwnedValue::try_from(value) {
            state.insert(key.to_owned(), v);
        }
    }
    state
}

/// The served interface.
#[derive(Debug)]
pub struct InhibitService {
    hub: SharedHub,
}

impl Default for InhibitService {
    fn default() -> Self {
        Self::with_effect(Box::new(LogindInhibitor::new()))
    }
}

impl InhibitService {
    /// Release every hold hung on `peer` (a unique name that left the bus, or stopped being the
    /// portal frontend) and take their request objects off the bus.
    pub async fn peer_gone(&self, peer: &str, connection: &zbus::Connection) {
        let handles = self.hub.lock().await.release_peer(peer).await;
        for handle in handles {
            let _ = connection
                .object_server()
                .remove::<RequestObject, _>(handle.0.as_str())
                .await;
        }
    }

    /// A service holding no requests, telling the system's login manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// A service holding no requests, telling `effect`.
    pub fn with_effect(effect: Box<dyn InhibitEffect>) -> Self {
        Self {
            hub: Arc::new(AsyncMutex::new(InhibitHub::new(effect))),
        }
    }
}

// The D-Bus interface lives in a private module: zbus's macro generates undocumented
// property-change methods, which the crate's `missing_docs` would flag on a public impl.
mod interface {
    use super::*;

    #[zbus::interface(name = "org.freedesktop.impl.portal.Inhibit")]
    impl InhibitService {
        /// Hold a request until its request object is closed.
        #[allow(clippy::too_many_arguments)] // the portal's own signature plus the bus context
        async fn inhibit(
            &self,
            handle: ObjectPath<'_>,
            app_id: &str,
            window: &str,
            flags: u32,
            options: Dict,
            #[zbus(object_server)] server: &ObjectServer,
            #[zbus(header)] header: zbus::message::Header<'_>,
            #[zbus(connection)] connection: &zbus::Connection,
        ) {
            let _ = (app_id, window, options);
            let key = RequestHandle(handle.to_string());
            let tether = Tether::of(connection, &header).await;
            if let Err(error) = self
                .hub
                .lock()
                .await
                .hold(key.clone(), InhibitFlags::from_wire(flags), tether)
                .await
            {
                tracing::warn!(%error, "a request to keep the session running was not held");
                return;
            }
            let request = RequestObject {
                hub: Arc::clone(&self.hub),
                key,
                path: OwnedObjectPath::from(handle.clone()),
            };
            if let Err(error) = server.at(handle.as_str(), request).await {
                tracing::warn!(%error, "the request object could not be exported");
            }
        }

        /// Start watching the session state (screen locked, idle); returns the response code.
        async fn create_monitor(
            &self,
            handle: ObjectPath<'_>,
            session_handle: ObjectPath<'_>,
            app_id: &str,
            window: &str,
            #[zbus(object_server)] server: &ObjectServer,
            #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
        ) -> u32 {
            let _ = (handle, app_id, window);
            let session = session_handle.to_string();
            if self.hub.lock().await.add_monitor(&session) != MonitorAdded::New {
                return 2;
            }
            let object = MonitorObject {
                hub: Arc::clone(&self.hub),
                session: session.clone(),
            };
            if let Err(error) = server.at(session_handle.as_str(), object).await {
                tracing::warn!(%error, "the monitor object could not be exported");
                self.hub.lock().await.remove_monitor(&session);
                return 2;
            }
            if let Err(error) =
                Self::state_changed(&emitter, session_handle.clone(), initial_state()).await
            {
                tracing::warn!(%error, "the first state could not be sent");
            }
            0
        }

        /// An app answered the end-of-session query.
        async fn query_end_response(&self, session_handle: ObjectPath<'_>) {
            let known = self.hub.lock().await.note_answer(session_handle.as_str());
            if known == MonitorKnown::Unknown {
                tracing::debug!("an answer for a session that is not monitored");
            }
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

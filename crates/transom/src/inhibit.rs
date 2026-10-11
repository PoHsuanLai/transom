//! `org.freedesktop.impl.portal.Inhibit`: keeping the screen on, or the session from ending.
//!
//! The decisions are `transom_core::InhibitBook`. [`InhibitHub`] holds the book together with the
//! effect (telling the session what is now held, behind [`InhibitEffect`]) and the monitors, so
//! the whole of it runs in tests against [`FakeEffect`]. The service object is a thin wrapper
//! that exports the portal's request and session objects.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use tokio::sync::Mutex as AsyncMutex;
use transom_core::{InhibitBook, InhibitFlags, RequestHandle};
use zbus::ObjectServer;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

use crate::BoxFuture;
use crate::screencast::Dict;

/// Why the session could not be told.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EffectError {
    /// The service that holds the inhibition refused or could not be reached.
    #[error("the session could not be told: {0}")]
    Session(String),
}

/// The seam: what holding a request does to the session. Called only when the union of held
/// flags changes, with the new union ([`InhibitFlags::NONE`] releases everything).
pub trait InhibitEffect: Send + std::fmt::Debug {
    /// Make the session hold exactly `active`.
    fn apply(&mut self, active: InhibitFlags) -> BoxFuture<'_, Result<(), EffectError>>;
}

/// The words logind uses for what to hold off, from the portal's flags. Logging out maps to
/// `shutdown`; switching users has no logind counterpart and is not held.
pub fn logind_what(active: InhibitFlags) -> String {
    [
        (InhibitFlags::LOGOUT, "shutdown"),
        (InhibitFlags::SUSPEND, "sleep"),
        (InhibitFlags::IDLE, "idle"),
    ]
    .iter()
    .filter(|(flag, _)| active.contains(*flag))
    .map(|(_, word)| *word)
    .collect::<Vec<_>>()
    .join(":")
}

/// The real effect: a blocking inhibitor lock taken from the system's login manager. The lock is
/// the file descriptor it hands back; dropping it releases.
#[derive(Debug, Default)]
pub struct LogindInhibitor {
    connection: Option<zbus::Connection>,
    lock: Option<zbus::zvariant::OwnedFd>,
}

impl LogindInhibitor {
    /// An inhibitor that connects to the system bus on first use.
    pub fn new() -> Self {
        Self::default()
    }

    async fn take(&mut self, active: InhibitFlags) -> Result<(), EffectError> {
        let session = |e: &dyn std::fmt::Display| EffectError::Session(e.to_string());
        let what = logind_what(active);
        if what.is_empty() {
            self.lock = None;
            return Ok(());
        }
        let connection = match &self.connection {
            Some(c) => c.clone(),
            None => {
                let c = zbus::Connection::system().await.map_err(|e| session(&e))?;
                self.connection = Some(c.clone());
                c
            }
        };
        let reply = connection
            .call_method(
                Some("org.freedesktop.login1"),
                "/org/freedesktop/login1",
                Some("org.freedesktop.login1.Manager"),
                "Inhibit",
                &(
                    what.as_str(),
                    "Apps",
                    "An app asked to keep the session running",
                    "block",
                ),
            )
            .await
            .map_err(|e| session(&e))?;
        let fd: zbus::zvariant::OwnedFd = reply.body().deserialize().map_err(|e| session(&e))?;
        // The new lock is held before the old one is let go, so there is no gap.
        self.lock = Some(fd);
        Ok(())
    }
}

impl InhibitEffect for LogindInhibitor {
    fn apply(&mut self, active: InhibitFlags) -> BoxFuture<'_, Result<(), EffectError>> {
        Box::pin(self.take(active))
    }
}

/// An effect for tests: records each union it was asked to hold.
#[derive(Debug, Default)]
pub struct FakeEffect {
    applied: Arc<Mutex<Vec<InhibitFlags>>>,
}

impl FakeEffect {
    /// An effect that has applied nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// The log of applied unions, shared with the effect, so a test can keep it after the effect
    /// moved into a hub.
    pub fn log(&self) -> Arc<Mutex<Vec<InhibitFlags>>> {
        Arc::clone(&self.applied)
    }
}

impl InhibitEffect for FakeEffect {
    fn apply(&mut self, active: InhibitFlags) -> BoxFuture<'_, Result<(), EffectError>> {
        self.applied
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(active);
        Box::pin(std::future::ready(Ok(())))
    }
}

/// What a monitoring session has done.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MonitorState {
    /// Whether the app answered the end-of-session query.
    pub answered_query_end: bool,
}

/// The held requests, the effect on the session and the monitors.
#[derive(Debug)]
pub struct InhibitHub {
    book: InhibitBook,
    effect: Box<dyn InhibitEffect>,
    monitors: BTreeMap<String, MonitorState>,
}

impl InhibitHub {
    /// A hub holding nothing, telling `effect` of every change.
    pub fn new(effect: Box<dyn InhibitEffect>) -> Self {
        Self {
            book: InhibitBook::new(),
            effect,
            monitors: BTreeMap::new(),
        }
    }

    /// Hold a request. The book is updated even when the effect fails (the error says so).
    pub async fn hold(
        &mut self,
        handle: RequestHandle,
        flags: InhibitFlags,
    ) -> Result<(), EffectError> {
        let change = self.book.hold(handle, flags);
        self.tell(change).await
    }

    /// Release a request; a handle not held does nothing.
    pub async fn release(&mut self, handle: &RequestHandle) -> Result<(), EffectError> {
        let change = self.book.release(handle);
        self.tell(change).await
    }

    async fn tell(&mut self, change: transom_core::BookChange) -> Result<(), EffectError> {
        if change.is_change() {
            self.effect.apply(change.after).await
        } else {
            Ok(())
        }
    }

    /// The union of what is held.
    pub fn active(&self) -> InhibitFlags {
        self.book.active()
    }

    /// How many requests are held.
    pub fn held(&self) -> usize {
        self.book.len()
    }

    /// Start a monitor; false when that session already has one.
    pub fn add_monitor(&mut self, session: &str) -> MonitorAdded {
        match self.monitors.entry(session.to_owned()) {
            std::collections::btree_map::Entry::Vacant(v) => {
                v.insert(MonitorState::default());
                MonitorAdded::New
            }
            std::collections::btree_map::Entry::Occupied(_) => MonitorAdded::AlreadyThere,
        }
    }

    /// Stop a monitor.
    pub fn remove_monitor(&mut self, session: &str) {
        self.monitors.remove(session);
    }

    /// Note that a monitor answered the end-of-session query; false for an unknown session.
    pub fn note_answer(&mut self, session: &str) -> MonitorKnown {
        match self.monitors.get_mut(session) {
            Some(state) => {
                state.answered_query_end = true;
                MonitorKnown::Known
            }
            None => MonitorKnown::Unknown,
        }
    }

    /// A monitor's state.
    pub fn monitor(&self, session: &str) -> Option<MonitorState> {
        self.monitors.get(session).copied()
    }
}

/// Whether a monitor was new.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MonitorAdded {
    /// It was started.
    New,
    /// The session was already monitored.
    AlreadyThere,
}

/// Whether a session is a monitor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MonitorKnown {
    /// It is.
    Known,
    /// It is not.
    Unknown,
}

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
        async fn inhibit(
            &self,
            handle: ObjectPath<'_>,
            app_id: &str,
            window: &str,
            flags: u32,
            options: Dict,
            #[zbus(object_server)] server: &ObjectServer,
        ) {
            let _ = (app_id, window, options);
            let key = RequestHandle(handle.to_string());
            if let Err(error) = self
                .hub
                .lock()
                .await
                .hold(key.clone(), InhibitFlags::from_wire(flags))
                .await
            {
                tracing::warn!(%error, "the session could not be told");
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
            if self.hub.lock().await.add_monitor(&session) == MonitorAdded::AlreadyThere {
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

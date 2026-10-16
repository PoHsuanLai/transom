//! `org.freedesktop.impl.portal.RemoteDesktop`: an app drives the keyboard, pointer and touch.
//!
//! Input goes to the compositor over EIS (`ConnectToEIS`), where it arrives tagged as remote so
//! it can never pass for the person's own keystrokes. The compositor half is casement's
//! `casement-agent/src/eis.rs`; the socket comes from `org.quire.Compositor1.ConnectEis`
//! ([`CasementEis`]). The `Notify*` methods are accepted and dropped: the only way in is the
//! EIS socket, which the person's consent opens.
//!
//! The person is asked in a question of its own that names the input the app would take over
//! ([`consent`]); a screen-sharing pick is never a yes. A session has at most one socket, and
//! closing the session takes it back from the compositor.
//!
//! The calls are decided by [`RemoteDesktopService`]'s public methods (`create`, `select`,
//! `begin`, `connect`, `close`), which the interface methods only wrap; tests drive them with
//! [`FakeConsent`] and [`FakeEis`] and need no bus.

mod book;
pub mod consent;
mod devices;
pub mod eis;
mod sessions;

use std::os::fd::OwnedFd;
use std::sync::Arc;

use zbus::fdo;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

pub use self::consent::{
    Consent, ConsentAnswer, ConsentError, FakeConsent, ShellConsent, remote_control_question,
};
use self::devices::DEVICE_TYPES;
pub use self::devices::DeviceKinds;
pub use self::eis::{CasementEis, EisError, EisHandout, FakeEis};
pub use self::sessions::ConnectError;
use self::sessions::{Core, Settled, Verdict};
use crate::names::AppNames;
use crate::screencast::Dict;
use crate::screencast::session::SessionKey;

/// The portal's response codes.
const GRANTED: u32 = 0;
const CANCELLED: u32 = 1;
const FAILED: u32 = 2;

/// The answer to a call: the portal's response code and its results.
pub type Reply = (u32, Dict);

/// The served interface.
#[derive(Debug)]
pub struct RemoteDesktopService {
    core: Arc<Core>,
    consent: Box<dyn Consent>,
    names: AppNames,
}

impl RemoteDesktopService {
    /// A service with no sessions, asking `consent` and getting sockets from `eis`. Apps are
    /// shown by their ids until [`RemoteDesktopService::with_names`] says where their names are.
    pub fn new(consent: Box<dyn Consent>, eis: Box<dyn EisHandout>) -> Self {
        Self {
            core: Arc::new(Core::new(eis)),
            consent,
            names: AppNames::none(),
        }
    }

    /// This service showing apps by the names `names` finds.
    pub fn with_names(mut self, names: AppNames) -> Self {
        self.names = names;
        self
    }

    /// Opens the session `key` for `app_id`. A handle that is already open fails.
    pub fn create(&self, key: SessionKey, app_id: &str) -> Reply {
        match self.core.create(key, app_id) {
            Some(_) => (GRANTED, Dict::new()),
            None => failed(),
        }
    }

    /// Records the `types` the app wants (all, when it says nothing). Only before `Start`.
    pub fn select(&self, key: &SessionKey, options: &Dict) -> Reply {
        let wanted = options
            .get("types")
            .and_then(|value| value.downcast_ref::<u32>().ok())
            .map_or(DeviceKinds::ALL, DeviceKinds::from_wire);
        match self.core.select(key, wanted) {
            Settled::Applied => (GRANTED, Dict::new()),
            Settled::Gone => failed(),
        }
    }

    /// Asks the person and, on a yes, answers with the devices granted. A no is response 1.
    /// Without anyone to ask nothing is granted (response 2). The yes is bound to this very
    /// session: one closed and opened again while the person decided does not inherit it.
    pub async fn begin(&self, key: &SessionKey) -> Reply {
        let Some(asking) = self.core.start_asking(key) else {
            return failed();
        };
        let label = self.names.label(&asking.app_id);
        let settle = |verdict| self.core.settle(key, asking.id, verdict);
        match self.consent.ask(&label, asking.kinds).await {
            Ok(ConsentAnswer::Allowed) => match settle(Verdict::Allow) {
                Settled::Applied => {
                    let mut results = Dict::new();
                    if let Some(devices) = owned(asking.kinds.to_wire()) {
                        results.insert("devices".to_owned(), devices);
                    }
                    (GRANTED, results)
                }
                // Closed while the person was deciding.
                Settled::Gone => (CANCELLED, Dict::new()),
            },
            Ok(_) => {
                settle(Verdict::Decline);
                (CANCELLED, Dict::new())
            }
            Err(error) => {
                tracing::warn!(%error, "remote control could not be asked about");
                settle(Verdict::Retract);
                failed()
            }
        }
    }

    /// The one socket the app speaks EIS on, once the person has allowed the session.
    pub async fn connect(&self, key: &SessionKey) -> Result<OwnedFd, ConnectError> {
        self.core.connect(key).await
    }

    /// Ends the session `key`, taking its socket back; an unknown one is ignored.
    pub async fn close(&self, key: &SessionKey) {
        self.core.close(key).await;
    }

    /// How many sessions are open.
    pub fn len(&self) -> usize {
        self.core.len()
    }

    /// Whether none are open.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The `Notify*` methods are not a way in: input comes only through the EIS socket.
    fn dropped(&self, method: &str) {
        tracing::debug!(
            method,
            "remote input sent outside the EIS socket was dropped"
        );
    }
}

fn failed() -> Reply {
    (FAILED, Dict::new())
}

fn owned(value: u32) -> Option<OwnedValue> {
    OwnedValue::try_from(Value::from(value)).ok()
}

impl From<ConnectError> for fdo::Error {
    fn from(error: ConnectError) -> Self {
        match error {
            ConnectError::NoSession => fdo::Error::UnknownObject(error.to_string()),
            ConnectError::NotAllowed | ConnectError::AlreadyConnected => {
                fdo::Error::AccessDenied(error.to_string())
            }
            ConnectError::Eis(_) => fdo::Error::Failed(error.to_string()),
        }
    }
}

/// The backend's end of a session, at the session handle: the frontend calls `Close` on it.
#[derive(Debug)]
struct SessionObject {
    core: Arc<Core>,
    key: SessionKey,
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Session")]
impl SessionObject {
    /// End the session and take its input socket back.
    async fn close(&self, #[zbus(object_server)] server: &zbus::ObjectServer) {
        self.core.close(&self.key).await;
        let _ = server.remove::<SessionObject, _>(self.key.0.as_str()).await;
    }

    /// The interface version.
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        1
    }
}

#[zbus::interface(name = "org.freedesktop.impl.portal.RemoteDesktop")]
impl RemoteDesktopService {
    /// Open a session.
    async fn create_session(
        &self,
        handle: ObjectPath<'_>,
        session_handle: ObjectPath<'_>,
        app_id: &str,
        options: Dict,
        #[zbus(object_server)] server: &zbus::ObjectServer,
    ) -> Reply {
        let _ = (handle, options);
        let key = SessionKey(session_handle.to_string());
        let reply = self.create(key.clone(), app_id);
        if reply.0 == GRANTED {
            let object = SessionObject {
                core: Arc::clone(&self.core),
                key: key.clone(),
            };
            // `Ok(false)`: another portal interface already serves this handle's session object,
            // which a combined session expects. A real failure leaves a session nobody can close.
            if let Err(error) = server.at(session_handle, object).await {
                tracing::warn!(%error, "the session object could not be served");
                self.close(&key).await;
                return failed();
            }
        }
        reply
    }

    /// Record which device kinds the app wants.
    async fn select_devices(
        &self,
        handle: ObjectPath<'_>,
        session_handle: ObjectPath<'_>,
        app_id: &str,
        options: Dict,
    ) -> Reply {
        let _ = (handle, app_id);
        self.select(&SessionKey(session_handle.to_string()), &options)
    }

    /// Ask the person to allow it.
    async fn start(
        &self,
        handle: ObjectPath<'_>,
        session_handle: ObjectPath<'_>,
        app_id: &str,
        parent_window: &str,
        options: Dict,
    ) -> Reply {
        let _ = (handle, app_id, parent_window, options);
        self.begin(&SessionKey(session_handle.to_string())).await
    }

    /// Relative pointer motion.
    async fn notify_pointer_motion(
        &self,
        session_handle: ObjectPath<'_>,
        options: Dict,
        dx: f64,
        dy: f64,
    ) {
        let _ = (session_handle, options, dx, dy);
        self.dropped("NotifyPointerMotion");
    }

    /// Pointer motion to a position in a stream.
    async fn notify_pointer_motion_absolute(
        &self,
        session_handle: ObjectPath<'_>,
        options: Dict,
        stream: u32,
        x: f64,
        y: f64,
    ) {
        let _ = (session_handle, options, stream, x, y);
        self.dropped("NotifyPointerMotionAbsolute");
    }

    /// A pointer button.
    async fn notify_pointer_button(
        &self,
        session_handle: ObjectPath<'_>,
        options: Dict,
        button: i32,
        state: u32,
    ) {
        let _ = (session_handle, options, button, state);
        self.dropped("NotifyPointerButton");
    }

    /// Smooth scrolling.
    async fn notify_pointer_axis(
        &self,
        session_handle: ObjectPath<'_>,
        options: Dict,
        dx: f64,
        dy: f64,
    ) {
        let _ = (session_handle, options, dx, dy);
        self.dropped("NotifyPointerAxis");
    }

    /// Wheel clicks.
    async fn notify_pointer_axis_discrete(
        &self,
        session_handle: ObjectPath<'_>,
        options: Dict,
        axis: u32,
        steps: i32,
    ) {
        let _ = (session_handle, options, axis, steps);
        self.dropped("NotifyPointerAxisDiscrete");
    }

    /// A key by its evdev code.
    async fn notify_keyboard_keycode(
        &self,
        session_handle: ObjectPath<'_>,
        options: Dict,
        keycode: i32,
        state: u32,
    ) {
        let _ = (session_handle, options, keycode, state);
        self.dropped("NotifyKeyboardKeycode");
    }

    /// A key by its keysym.
    async fn notify_keyboard_keysym(
        &self,
        session_handle: ObjectPath<'_>,
        options: Dict,
        keysym: i32,
        state: u32,
    ) {
        let _ = (session_handle, options, keysym, state);
        self.dropped("NotifyKeyboardKeysym");
    }

    /// A touch begins.
    async fn notify_touch_down(
        &self,
        session_handle: ObjectPath<'_>,
        options: Dict,
        stream: u32,
        slot: u32,
        x: f64,
        y: f64,
    ) {
        let _ = (session_handle, options, stream, slot, x, y);
        self.dropped("NotifyTouchDown");
    }

    /// A touch moves.
    async fn notify_touch_motion(
        &self,
        session_handle: ObjectPath<'_>,
        options: Dict,
        stream: u32,
        slot: u32,
        x: f64,
        y: f64,
    ) {
        let _ = (session_handle, options, stream, slot, x, y);
        self.dropped("NotifyTouchMotion");
    }

    /// A touch ends.
    async fn notify_touch_up(&self, session_handle: ObjectPath<'_>, options: Dict, slot: u32) {
        let _ = (session_handle, options, slot);
        self.dropped("NotifyTouchUp");
    }

    /// Hand the app its EIS socket for this session (one per session).
    async fn connect_to_eis(
        &self,
        session_handle: ObjectPath<'_>,
        app_id: &str,
        options: Dict,
    ) -> fdo::Result<zbus::zvariant::OwnedFd> {
        let _ = (app_id, options);
        let fd = self
            .connect(&SessionKey(session_handle.to_string()))
            .await?;
        Ok(zbus::zvariant::OwnedFd::from(fd))
    }

    /// The device kinds served.
    #[zbus(property)]
    fn available_device_types(&self) -> u32 {
        DEVICE_TYPES
    }

    /// The interface version.
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        2
    }
}

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
//! a `FakeAsker` and [`FakeEis`] and need no bus.

mod book;
pub mod consent;
pub mod eis;
mod object;
mod sessions;

use std::os::fd::OwnedFd;
use std::sync::Arc;

use transom_core::{DEVICE_TYPES, PersistMode};
use zbus::fdo;
use zbus::message::Header;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

pub use self::consent::remote_control_question;
pub use self::eis::{CasementEis, EisError, EisHandout, FakeEis};
use self::object::SessionObject;
pub use self::sessions::ConnectError;
use self::sessions::{Asking, Core, Settled, Verdict, Wanted};
use crate::access::{Answer, Asker};
use crate::app_proof::{AppProver, Who};
use crate::keeper::{Keeper, Kept, unix_now};
use crate::names::AppNames;
use crate::peers::{RoleGuard, Tether};
use crate::response::Response;
use crate::screencast::Dict;
use crate::screencast::options::{restore_data, restore_token};
use crate::table::{Refusal, SessionKey};
pub use transom_core::DeviceKinds;

/// The answer to a call: the portal's response code and its results.
pub type Reply = (u32, Dict);

/// What lets the service remember a yes: the book, and who says which apps are what they claim.
#[derive(Debug)]
struct Memory {
    keeper: Arc<Keeper>,
    prover: Arc<dyn AppProver>,
}

/// The served interface.
#[derive(Debug)]
pub struct RemoteDesktopService {
    core: Arc<Core>,
    asker: Box<dyn Asker>,
    names: AppNames,
    guard: RoleGuard,
    /// Without it nothing is remembered and every session is asked about.
    memory: Option<Memory>,
}

impl RemoteDesktopService {
    /// A service with no sessions, asking the person through `asker` and getting sockets from
    /// `eis`. Apps are shown by their ids until [`RemoteDesktopService::with_names`] says where
    /// their names are.
    pub fn new(asker: Box<dyn Asker>, eis: Box<dyn EisHandout>) -> Self {
        Self {
            core: Arc::new(Core::new(eis)),
            asker,
            names: AppNames::none(),
            guard: RoleGuard::default(),
            memory: None,
        }
    }

    /// This service remembering a yes in `keeper` for the apps `prover` shows to be what their
    /// ids say (or, for an app with no id, which app it is), and no others. Use the screen-sharing
    /// service's keeper and prover, so both share one book and one list. Without it nothing is
    /// remembered.
    pub fn with_memory(mut self, keeper: Arc<Keeper>, prover: Arc<dyn AppProver>) -> Self {
        self.memory = Some(Memory { keeper, prover });
        self
    }

    /// This service letting through the callers `guard` admits. Without it, no wire call passes.
    pub fn with_guard(mut self, guard: RoleGuard) -> Self {
        self.guard = guard;
        self
    }

    /// Let the call with `header` through if it is from the portal frontend and, when the session
    /// `key` is open, from the peer that opened it.
    async fn admit_for(&self, header: &Header<'_>, key: &SessionKey) -> fdo::Result<()> {
        match self.core.creator(key) {
            Some(creator) => self.guard.admit_creator(header, &creator).await,
            None => self.guard.admit(header).await.map(|_| ()),
        }
    }

    /// This service showing apps by the names `names` finds.
    pub fn with_names(mut self, names: AppNames) -> Self {
        self.names = names;
        self
    }

    /// Opens the session `key` for `app_id`, hung on no peer. A handle that is already open
    /// fails, and so does a full book.
    pub fn create(&self, key: SessionKey, app_id: &str) -> Reply {
        match self.create_for(key, app_id, Tether::default()) {
            Ok(_) => Response::granted().into_wire(),
            Err(_) => failed(),
        }
    }

    /// Opens the session `key` for `app_id`, hung on `tether`: it ends when either peer leaves the
    /// bus. The result is the handles dropped to make room (sessions that never began), whose
    /// session objects the caller takes off the bus.
    pub fn create_for(
        &self,
        key: SessionKey,
        app_id: &str,
        tether: Tether,
    ) -> Result<Vec<SessionKey>, Refusal> {
        self.core.create(key, app_id, tether)
    }

    /// Called once, as the portal starts: the compositor drops every input socket an earlier run
    /// handed out. This run's book is empty, so an app of the earlier run that still holds one
    /// would keep its input with no session, no sheet and no way to end it (NEW-PI-8).
    pub async fn forget_previous_run(&self) {
        self.core.end_all().await;
    }

    /// End every session hung on `peer` (a unique name that left the bus, or stopped being the
    /// portal frontend), taking back the sockets handed out; the handles ended.
    pub async fn release_peer(&self, peer: &str) -> Vec<SessionKey> {
        self.core.peer_gone(peer).await
    }

    /// As [`RemoteDesktopService::release_peer`], and take the sessions' objects off the bus.
    pub async fn peer_gone(&self, peer: &str, connection: &zbus::Connection) {
        for key in self.release_peer(peer).await {
            let _ = connection
                .object_server()
                .remove::<SessionObject, _>(key.0.as_str())
                .await;
        }
    }

    /// Records the `types` the app wants (all, when it says nothing), how long it asks for the
    /// yes to be remembered (`persist_mode`, not at all when it says nothing) and the token it
    /// offers (`restore_data`). Only before `Start`.
    pub fn select(&self, key: &SessionKey, options: &Dict) -> Reply {
        let number = |name: &str| {
            options
                .get(name)
                .and_then(|value| value.downcast_ref::<u32>().ok())
        };
        let wanted = Wanted {
            kinds: number("types").map_or(DeviceKinds::ALL, DeviceKinds::from_wire),
            persist: number("persist_mode")
                .and_then(PersistMode::from_wire)
                .unwrap_or(PersistMode::DoNot),
            restore: restore_token(options),
        };
        match self.core.select(key, wanted) {
            Settled::Applied => Response::granted().into_wire(),
            Settled::Gone => failed(),
        }
    }

    /// Asks the person and, on a yes, answers with the devices granted. A no is response 1.
    /// Without anyone to ask nothing is granted (response 2). The yes is bound to this very
    /// session: one closed and opened again while the person decided does not inherit it.
    ///
    /// An app that is shown to be what it says, and offers the token of an earlier yes for the
    /// same devices or fewer, is not asked. One that asks to be remembered gets a token with its
    /// yes.
    pub async fn begin(&self, key: &SessionKey) -> Reply {
        let Some(asking) = self.core.start_asking(key) else {
            return failed();
        };
        let who = self.identify(key, &asking.app_id).await;
        let settle = |verdict| self.core.settle(key, asking.id, verdict);
        let restored = self.restored_kinds(&asking, &who).await;
        let answer = if restored.is_empty() {
            let label = self.names.label(&who.shown);
            let question = remote_control_question(&label, asking.kinds);
            self.asker.ask(&question).await
        } else {
            Ok(Answer::Granted)
        };
        match answer {
            Ok(Answer::Granted) => match settle(Verdict::Allow) {
                Settled::Applied => {
                    let mut results = Dict::new();
                    if let Some(devices) = owned(asking.kinds.to_wire()) {
                        results.insert("devices".to_owned(), devices);
                    }
                    // It is remembered for all the earlier yes covered, not only what this call asked for.
                    let covered = if restored.is_empty() {
                        asking.kinds
                    } else {
                        restored
                    };
                    self.keep(&asking, &who, covered, &mut results).await;
                    Response::Granted(results).into_wire()
                }
                // Closed while the person was deciding.
                Settled::Gone => Response::Cancelled.into_wire(),
            },
            Ok(_) => {
                settle(Verdict::Decline);
                Response::Cancelled.into_wire()
            }
            Err(error) => {
                tracing::warn!(%error, "remote control could not be asked about");
                settle(Verdict::Retract);
                failed()
            }
        }
    }

    /// Who the app of the session `key` is, as far as it is shown: an app is shown to be what it
    /// says only when the service can remember, and its prover says so.
    async fn identify(&self, key: &SessionKey, app_id: &str) -> Who {
        match &self.memory {
            Some(memory) => Who::identify(memory.prover.as_ref(), &key.0, app_id).await,
            None => Who {
                shown: app_id.to_owned(),
                proven: None,
            },
        }
    }

    /// The kinds the earlier yes offered by `asking` covers, if it covers all those wanted; else
    /// none.
    async fn restored_kinds(&self, asking: &Asking, who: &Who) -> DeviceKinds {
        let (Some(memory), Some(token), Some(app_id)) =
            (&self.memory, asking.restore.as_ref(), who.proven.as_deref())
        else {
            return DeviceKinds::NONE;
        };
        memory
            .keeper
            .book()
            .await
            .resolve_input(token, app_id, asking.kinds, unix_now())
    }

    /// Remember the yes when the app asked to be remembered and is shown to be what it says, and
    /// put the new token in `results`. It is remembered for the `covered` kinds, and the token it
    /// replaces is forgotten.
    async fn keep(&self, asking: &Asking, who: &Who, covered: DeviceKinds, results: &mut Dict) {
        let (Some(memory), PersistMode::Keep(keep), Some(app_id)) =
            (&self.memory, asking.persist, who.proven.as_deref())
        else {
            return;
        };
        let kept = Kept::Input(covered);
        let Some(token) = memory
            .keeper
            .keep(app_id, kept, keep, asking.restore.clone())
            .await
        else {
            return;
        };
        if let Ok(data) = restore_data(&token)
            && let Some(mode) = owned(keep.to_wire())
        {
            results.insert("persist_mode".to_owned(), mode);
            results.insert("restore_data".to_owned(), data);
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

    /// The `Notify*` methods are not a way in: input comes only through the EIS socket. A call
    /// is still refused unless it is from the peer that opened the session.
    async fn dropped(
        &self,
        method: &str,
        session: &ObjectPath<'_>,
        header: &Header<'_>,
    ) -> fdo::Result<()> {
        self.admit_for(header, &SessionKey(session.to_string()))
            .await?;
        tracing::debug!(
            method,
            "remote input sent outside the EIS socket was dropped"
        );
        Ok(())
    }
}

fn failed() -> Reply {
    Response::Failed.into_wire()
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

#[zbus::interface(name = "org.freedesktop.impl.portal.RemoteDesktop")]
impl RemoteDesktopService {
    /// Open a session.
    #[allow(clippy::too_many_arguments)] // the portal's own signature plus the bus context
    async fn create_session(
        &self,
        handle: ObjectPath<'_>,
        session_handle: ObjectPath<'_>,
        app_id: &str,
        options: Dict,
        #[zbus(object_server)] server: &zbus::ObjectServer,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> fdo::Result<Reply> {
        let _ = (handle, options);
        self.guard.admit(&header).await?;
        let key = SessionKey(session_handle.to_string());
        let tether = Tether::of(connection, &header).await;
        let reply = match self.create_for(key.clone(), app_id, tether) {
            Ok(dropped) => {
                for old in dropped {
                    let _ = server.remove::<SessionObject, _>(old.0.as_str()).await;
                }
                Response::granted().into_wire()
            }
            Err(_) => failed(),
        };
        if reply.0 == Response::GRANTED {
            let object =
                SessionObject::new(Arc::clone(&self.core), key.clone(), self.guard.clone());
            // `Ok(false)`: another portal interface already serves this handle's session object,
            // which a combined session expects. A real failure leaves a session nobody can close.
            if let Err(error) = server.at(session_handle, object).await {
                tracing::warn!(%error, "the session object could not be served");
                self.close(&key).await;
                return Ok(failed());
            }
        }
        Ok(reply)
    }

    /// Record which device kinds the app wants.
    async fn select_devices(
        &self,
        handle: ObjectPath<'_>,
        session_handle: ObjectPath<'_>,
        app_id: &str,
        options: Dict,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<Reply> {
        let _ = (handle, app_id);
        let key = SessionKey(session_handle.to_string());
        self.admit_for(&header, &key).await?;
        Ok(self.select(&key, &options))
    }

    /// Ask the person to allow it.
    async fn start(
        &self,
        handle: ObjectPath<'_>,
        session_handle: ObjectPath<'_>,
        app_id: &str,
        parent_window: &str,
        options: Dict,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<Reply> {
        let _ = (handle, app_id, parent_window, options);
        let key = SessionKey(session_handle.to_string());
        self.admit_for(&header, &key).await?;
        Ok(self.begin(&key).await)
    }

    /// Relative pointer motion.
    async fn notify_pointer_motion(
        &self,
        session_handle: ObjectPath<'_>,
        options: Dict,
        dx: f64,
        dy: f64,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        let _ = (options, dx, dy);
        self.dropped("NotifyPointerMotion", &session_handle, &header)
            .await
    }

    /// Pointer motion to a position in a stream.
    async fn notify_pointer_motion_absolute(
        &self,
        session_handle: ObjectPath<'_>,
        options: Dict,
        stream: u32,
        x: f64,
        y: f64,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        let _ = (options, stream, x, y);
        self.dropped("NotifyPointerMotionAbsolute", &session_handle, &header)
            .await
    }

    /// A pointer button.
    async fn notify_pointer_button(
        &self,
        session_handle: ObjectPath<'_>,
        options: Dict,
        button: i32,
        state: u32,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        let _ = (options, button, state);
        self.dropped("NotifyPointerButton", &session_handle, &header)
            .await
    }

    /// Smooth scrolling.
    async fn notify_pointer_axis(
        &self,
        session_handle: ObjectPath<'_>,
        options: Dict,
        dx: f64,
        dy: f64,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        let _ = (options, dx, dy);
        self.dropped("NotifyPointerAxis", &session_handle, &header)
            .await
    }

    /// Wheel clicks.
    async fn notify_pointer_axis_discrete(
        &self,
        session_handle: ObjectPath<'_>,
        options: Dict,
        axis: u32,
        steps: i32,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        let _ = (options, axis, steps);
        self.dropped("NotifyPointerAxisDiscrete", &session_handle, &header)
            .await
    }

    /// A key by its evdev code.
    async fn notify_keyboard_keycode(
        &self,
        session_handle: ObjectPath<'_>,
        options: Dict,
        keycode: i32,
        state: u32,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        let _ = (options, keycode, state);
        self.dropped("NotifyKeyboardKeycode", &session_handle, &header)
            .await
    }

    /// A key by its keysym.
    async fn notify_keyboard_keysym(
        &self,
        session_handle: ObjectPath<'_>,
        options: Dict,
        keysym: i32,
        state: u32,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        let _ = (options, keysym, state);
        self.dropped("NotifyKeyboardKeysym", &session_handle, &header)
            .await
    }

    /// A touch begins.
    #[allow(clippy::too_many_arguments)] // the portal's own signature plus the bus context
    async fn notify_touch_down(
        &self,
        session_handle: ObjectPath<'_>,
        options: Dict,
        stream: u32,
        slot: u32,
        x: f64,
        y: f64,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        let _ = (options, stream, slot, x, y);
        self.dropped("NotifyTouchDown", &session_handle, &header)
            .await
    }

    /// A touch moves.
    #[allow(clippy::too_many_arguments)] // the portal's own signature plus the bus context
    async fn notify_touch_motion(
        &self,
        session_handle: ObjectPath<'_>,
        options: Dict,
        stream: u32,
        slot: u32,
        x: f64,
        y: f64,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        let _ = (options, stream, slot, x, y);
        self.dropped("NotifyTouchMotion", &session_handle, &header)
            .await
    }

    /// A touch ends.
    async fn notify_touch_up(
        &self,
        session_handle: ObjectPath<'_>,
        options: Dict,
        slot: u32,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        let _ = (options, slot);
        self.dropped("NotifyTouchUp", &session_handle, &header)
            .await
    }

    /// Hand the app its EIS socket for this session (one per session). The bus name is spelled
    /// out: the derived one would be `ConnectToEis`, which no portal frontend calls.
    #[zbus(name = "ConnectToEIS")]
    async fn connect_to_eis(
        &self,
        session_handle: ObjectPath<'_>,
        app_id: &str,
        options: Dict,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<zbus::zvariant::OwnedFd> {
        let _ = (app_id, options);
        let key = SessionKey(session_handle.to_string());
        self.admit_for(&header, &key).await?;
        let fd = self.connect(&key).await?;
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

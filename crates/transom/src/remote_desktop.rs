//! `org.freedesktop.impl.portal.RemoteDesktop`: an app drives the keyboard, pointer and touch.
//!
//! Input goes to the compositor over EIS (`ConnectToEIS`), where it arrives tagged as remote so
//! it can never pass for the person's own keystrokes. The compositor half is casement's
//! `casement-agent/src/eis.rs`; the socket comes from `org.quire.Compositor1.ConnectEis`
//! ([`CasementEis`]). The `Notify*` methods are accepted and dropped: the only way in is the
//! EIS socket, which the person's consent opens.
//!
//! The calls are decided by [`RemoteDesktopService`]'s public methods (`create`, `select`,
//! `begin`, `connect`, `close`), which the interface methods only wrap; tests drive them with
//! [`FakeConsent`] and [`FakeEis`] and need no bus.

use std::collections::BTreeMap;
use std::fmt;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex, PoisonError};

use transom_core::choice::{Multiple, SourceKinds};
use transom_core::{ChoiceRequest, CursorMode};
use zbus::fdo;
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

use crate::BoxFuture;
use crate::chooser::{Chooser, ChooserError};
use crate::link::BusLink;
use crate::names::AppNames;
use crate::screencast::Dict;
use crate::screencast::session::SessionKey;

/// The device kinds an app may ask for: keyboard, pointer, touchscreen (1, 2, 4).
const DEVICE_TYPES: u32 = 7;

/// The portal's response codes.
const GRANTED: u32 = 0;
const CANCELLED: u32 = 1;
const FAILED: u32 = 2;

/// The device kinds of a session, as the portal's `types` bit set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DeviceKinds(u32);

impl DeviceKinds {
    /// Every kind this backend offers.
    pub const ALL: Self = Self(DEVICE_TYPES);

    /// The kinds a portal value names; bits this backend does not offer are dropped.
    pub fn from_wire(value: u32) -> Self {
        Self(value & DEVICE_TYPES)
    }

    /// The portal's value.
    pub fn to_wire(self) -> u32 {
        self.0
    }
}

/// What the person said to "let this app control the computer".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConsentAnswer {
    /// Allowed.
    Allowed,
    /// Not allowed (or the question was dismissed).
    Declined,
}

/// Why the person could not be asked.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConsentError {
    /// Nobody can be asked, so nothing is granted.
    #[error("the person could not be asked: {0}")]
    Unavailable(#[from] ChooserError),
}

/// The seam: ask the person whether `app_label` may control the computer with `kinds`.
pub trait Consent: Send + Sync + fmt::Debug {
    /// Ask and wait.
    fn ask<'a>(
        &'a self,
        app_label: &'a str,
        kinds: DeviceKinds,
    ) -> BoxFuture<'a, Result<ConsentAnswer, ConsentError>>;
}

impl<T: Consent + ?Sized> Consent for Arc<T> {
    fn ask<'a>(
        &'a self,
        app_label: &'a str,
        kinds: DeviceKinds,
    ) -> BoxFuture<'a, Result<ConsentAnswer, ConsentError>> {
        (**self).ask(app_label, kinds)
    }
}

/// The real consent: the shell's picker, through the chooser path. The shell has no dedicated
/// "allow remote control" question yet; any pick is a yes and a cancel is a no. Until it has,
/// a person sees the picker the sharing dialog uses.
#[derive(Debug)]
pub struct ShellConsent {
    chooser: Box<dyn Chooser>,
}

impl ShellConsent {
    /// Consent asked through `chooser`.
    pub fn new(chooser: Box<dyn Chooser>) -> Self {
        Self { chooser }
    }
}

impl Consent for ShellConsent {
    fn ask<'a>(
        &'a self,
        app_label: &'a str,
        _kinds: DeviceKinds,
    ) -> BoxFuture<'a, Result<ConsentAnswer, ConsentError>> {
        Box::pin(async move {
            let request = ChoiceRequest {
                kinds: SourceKinds::OUTPUTS,
                multiple: Multiple::One,
                cursor_modes: vec![CursorMode::Hidden],
                app_label: app_label.to_owned(),
            };
            Ok(match self.chooser.pick(&request).await? {
                Some(_) => ConsentAnswer::Allowed,
                None => ConsentAnswer::Declined,
            })
        })
    }
}

/// A consent that answers from a script; when the script is empty the person declines.
#[derive(Debug, Default)]
pub struct FakeConsent {
    answers: Mutex<Vec<ConsentAnswer>>,
    asked: Mutex<Vec<(String, DeviceKinds)>>,
}

impl FakeConsent {
    /// A consent that will give these answers in order.
    pub fn new(answers: Vec<ConsentAnswer>) -> Self {
        let mut answers = answers;
        answers.reverse();
        Self {
            answers: Mutex::new(answers),
            asked: Mutex::new(Vec::new()),
        }
    }

    /// The questions asked so far: the app's label and the kinds.
    pub fn asked(&self) -> Vec<(String, DeviceKinds)> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Consent for FakeConsent {
    fn ask<'a>(
        &'a self,
        app_label: &'a str,
        kinds: DeviceKinds,
    ) -> BoxFuture<'a, Result<ConsentAnswer, ConsentError>> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((app_label.to_owned(), kinds));
        let next = self
            .answers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop()
            .unwrap_or(ConsentAnswer::Declined);
        Box::pin(async move { Ok(next) })
    }
}

/// Why an EIS socket could not be had.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EisError {
    /// The compositor is not on the bus.
    #[error("the compositor is not available")]
    Unavailable,
    /// The compositor refused or the call failed.
    #[error("the compositor did not hand out a connection: {0}")]
    Call(String),
}

/// The seam: a socket the app speaks EIS on, for one session.
pub trait EisHandout: Send + Sync + fmt::Debug {
    /// The client end of an EIS socket for portal session `session` offering `kinds`.
    fn connect<'a>(
        &'a self,
        session: u64,
        kinds: DeviceKinds,
    ) -> BoxFuture<'a, Result<OwnedFd, EisError>>;
}

impl<T: EisHandout + ?Sized> EisHandout for Arc<T> {
    fn connect<'a>(
        &'a self,
        session: u64,
        kinds: DeviceKinds,
    ) -> BoxFuture<'a, Result<OwnedFd, EisError>> {
        (**self).connect(session, kinds)
    }
}

/// The compositor's side of the handshake, as the client sees it.
///
/// `ConnectEis(session: t, devices: u) -> (h)`: only this portal's unit may call it. The reply
/// is the client end of an EIS socket whose input the compositor tags as remote input of
/// `session`; `devices` is the portal's `types` bit set.
#[zbus::proxy(
    interface = "org.quire.Compositor1",
    default_service = "org.quire.Compositor1",
    default_path = "/org/quire/Compositor1",
    gen_blocking = false
)]
pub trait Compositor1 {
    /// An EIS socket for a remote-desktop session.
    fn connect_eis(&self, session: u64, devices: u32) -> zbus::Result<zbus::zvariant::OwnedFd>;
}

/// The real hand-out: `org.quire.Compositor1.ConnectEis` over the session bus, asked from the
/// connection that owns the portal's name (the compositor tells its caller by that connection).
#[derive(Debug)]
pub struct CasementEis {
    link: BusLink,
}

impl CasementEis {
    /// A hand-out that asks through `link`.
    pub fn new(link: BusLink) -> Self {
        Self { link }
    }
}

impl EisHandout for CasementEis {
    fn connect<'a>(
        &'a self,
        session: u64,
        kinds: DeviceKinds,
    ) -> BoxFuture<'a, Result<OwnedFd, EisError>> {
        Box::pin(async move {
            let call = |error: zbus::Error| EisError::Call(error.to_string());
            let connection = self
                .link
                .connection()
                .map_err(|error| EisError::Call(error.to_string()))?;
            let proxy = Compositor1Proxy::new(&connection).await.map_err(call)?;
            match proxy.connect_eis(session, kinds.to_wire()).await {
                Ok(fd) => Ok(OwnedFd::from(fd)),
                Err(zbus::Error::MethodError(name, ..))
                    if name.as_str().ends_with("ServiceUnknown")
                        || name.as_str().ends_with("NameHasNoOwner") =>
                {
                    Err(EisError::Unavailable)
                }
                Err(error) => Err(call(error)),
            }
        })
    }
}

/// A hand-out that makes socket pairs: it keeps the server ends so a test can talk through
/// them, and records what was asked.
#[derive(Debug, Default)]
pub struct FakeEis {
    asked: Mutex<Vec<(u64, DeviceKinds)>>,
    servers: Mutex<Vec<UnixStream>>,
}

impl FakeEis {
    /// A hand-out with nothing asked yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// The asks so far: the session number and the kinds.
    pub fn asked(&self) -> Vec<(u64, DeviceKinds)> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Takes the server end of the most recent socket.
    pub fn take_server(&self) -> Option<UnixStream> {
        self.servers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop()
    }
}

impl EisHandout for FakeEis {
    fn connect<'a>(
        &'a self,
        session: u64,
        kinds: DeviceKinds,
    ) -> BoxFuture<'a, Result<OwnedFd, EisError>> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((session, kinds));
        let made = UnixStream::pair().map_err(|error| EisError::Call(error.to_string()));
        Box::pin(async move {
            let (server, client) = made?;
            self.servers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(server);
            Ok(OwnedFd::from(client))
        })
    }
}

/// How far a session has come.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    /// Created; the app is choosing devices.
    Created,
    /// The person allowed it.
    Started,
}

/// One session.
#[derive(Clone, Debug)]
struct Entry {
    /// The number the compositor tags this session's input with.
    id: u64,
    app_id: String,
    kinds: DeviceKinds,
    stage: Stage,
}

/// Every open session.
#[derive(Debug, Default)]
struct Book {
    next: u64,
    entries: BTreeMap<SessionKey, Entry>,
}

/// Why a `ConnectToEIS` was refused.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConnectError {
    /// There is no such session.
    #[error("there is no such session")]
    NoSession,
    /// The person has not allowed it yet.
    #[error("the person has not allowed this yet")]
    NotAllowed,
    /// The compositor would not hand out a connection.
    #[error(transparent)]
    Eis(#[from] EisError),
}

impl From<ConnectError> for fdo::Error {
    fn from(error: ConnectError) -> Self {
        match error {
            ConnectError::NoSession => fdo::Error::UnknownObject(error.to_string()),
            ConnectError::NotAllowed => fdo::Error::AccessDenied(error.to_string()),
            ConnectError::Eis(_) => fdo::Error::Failed(error.to_string()),
        }
    }
}

/// The answer to a call: the portal's response code and its results.
pub type Reply = (u32, Dict);

/// The served interface.
#[derive(Debug)]
pub struct RemoteDesktopService {
    book: Arc<Mutex<Book>>,
    consent: Box<dyn Consent>,
    eis: Box<dyn EisHandout>,
    names: AppNames,
}

impl RemoteDesktopService {
    /// A service with no sessions, asking `consent` and getting sockets from `eis`. Apps are
    /// shown by their ids until [`RemoteDesktopService::with_names`] says where their names are.
    pub fn new(consent: Box<dyn Consent>, eis: Box<dyn EisHandout>) -> Self {
        Self {
            book: Arc::default(),
            consent,
            eis,
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
        let mut book = lock(&self.book);
        if book.entries.contains_key(&key) {
            return failed();
        }
        book.next += 1;
        let entry = Entry {
            id: book.next,
            app_id: app_id.to_owned(),
            kinds: DeviceKinds::ALL,
            stage: Stage::Created,
        };
        book.entries.insert(key, entry);
        (GRANTED, Dict::new())
    }

    /// Records the `types` the app wants (all, when it says nothing). Only before `Start`.
    pub fn select(&self, key: &SessionKey, options: &Dict) -> Reply {
        let wanted = options
            .get("types")
            .and_then(|value| value.downcast_ref::<u32>().ok())
            .map_or(DeviceKinds::ALL, DeviceKinds::from_wire);
        let mut book = lock(&self.book);
        match book.entries.get_mut(key) {
            Some(entry) if entry.stage == Stage::Created => {
                entry.kinds = wanted;
                (GRANTED, Dict::new())
            }
            _ => failed(),
        }
    }

    /// Asks the person and, on a yes, answers with the devices granted. A no is response 1.
    /// Without anyone to ask nothing is granted (response 2).
    pub async fn begin(&self, key: &SessionKey) -> Reply {
        let asked = lock(&self.book)
            .entries
            .get(key)
            .filter(|entry| entry.stage == Stage::Created)
            .map(|entry| (entry.app_id.clone(), entry.kinds));
        let Some((app_id, kinds)) = asked else {
            return failed();
        };
        let label = self.names.label(&app_id);
        match self.consent.ask(&label, kinds).await {
            Ok(ConsentAnswer::Allowed) => {}
            Ok(_) => {
                self.close(key);
                return (CANCELLED, Dict::new());
            }
            Err(_) => return failed(),
        }
        let mut book = lock(&self.book);
        let Some(entry) = book.entries.get_mut(key) else {
            // Closed while the person was deciding.
            return (CANCELLED, Dict::new());
        };
        entry.stage = Stage::Started;
        let mut results = Dict::new();
        if let Some(devices) = owned(kinds.to_wire()) {
            results.insert("devices".to_owned(), devices);
        }
        (GRANTED, results)
    }

    /// A socket the app speaks EIS on, once the person has allowed the session.
    pub async fn connect(&self, key: &SessionKey) -> Result<OwnedFd, ConnectError> {
        let (id, kinds) = {
            let book = lock(&self.book);
            let entry = book.entries.get(key).ok_or(ConnectError::NoSession)?;
            if entry.stage != Stage::Started {
                return Err(ConnectError::NotAllowed);
            }
            (entry.id, entry.kinds)
        };
        Ok(self.eis.connect(id, kinds).await?)
    }

    /// Ends the session `key`; an unknown one is ignored.
    pub fn close(&self, key: &SessionKey) {
        lock(&self.book).entries.remove(key);
    }

    /// How many sessions are open.
    pub fn len(&self) -> usize {
        lock(&self.book).entries.len()
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

fn lock(book: &Mutex<Book>) -> std::sync::MutexGuard<'_, Book> {
    book.lock().unwrap_or_else(PoisonError::into_inner)
}

fn failed() -> Reply {
    (FAILED, Dict::new())
}

fn owned(value: u32) -> Option<OwnedValue> {
    OwnedValue::try_from(Value::from(value)).ok()
}

/// The backend's end of a session, at the session handle: the frontend calls `Close` on it.
#[derive(Debug)]
struct SessionObject {
    book: Arc<Mutex<Book>>,
    key: SessionKey,
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Session")]
impl SessionObject {
    /// End the session.
    async fn close(&self, #[zbus(object_server)] server: &zbus::ObjectServer) {
        lock(&self.book).entries.remove(&self.key);
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
                book: self.book.clone(),
                key,
            };
            // Another portal interface may already serve this handle's session object.
            let _ = server.at(session_handle, object).await;
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

    /// Hand the app an EIS socket for this session.
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

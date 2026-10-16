//! The life of a screen-cast session: what happens between the wire calls.
//!
//! [`Shared`] holds the open sessions, the running shares, the chooser and the token store. The
//! decisions are `transom_core::step`; this runs the outputs it asks for.

use std::collections::{BTreeMap, VecDeque};
use std::io::Read;
use std::sync::{Arc, Mutex, MutexGuard};

use transom_core::{
    CastIn, CastOut, CastReply, CastSource, Choice, ChoiceRequest, Grant, PersistMode, PwNode,
    RestoreBook, RestoreToken,
};
use zbus::Connection;

use super::options;
use super::restore::RestoreStore;
use super::run::Share;
use super::session::{Selection, SessionBook, SessionKey};
use super::{Dict, SessionObject};
use crate::capture::{self, DisplayName};
use crate::chooser::{Chooser, unavailable_is_cancel};
use crate::names::AppNames;

/// The answer to a Start call, before it becomes wire values.
#[derive(Debug)]
pub(super) enum Outcome {
    /// Sharing began; the reply's results.
    Granted(Dict),
    /// The person said no.
    Cancelled,
    /// Something broke.
    Failed,
}

impl Outcome {
    /// The portal's response code and results: 0 granted, 1 cancelled, 2 failed.
    pub fn into_wire(self) -> (u32, Dict) {
        match self {
            Self::Granted(results) => (0, results),
            Self::Cancelled => (1, Dict::new()),
            Self::Failed => (2, Dict::new()),
        }
    }
}

/// Everything the interface and the session objects share.
#[derive(Debug)]
pub(super) struct Shared {
    sessions: Mutex<SessionBook>,
    shares: Mutex<BTreeMap<SessionKey, Share>>,
    chooser: Box<dyn Chooser>,
    restore: Box<dyn RestoreStore>,
    /// Held while the token book is read, changed and written, so two Starts do not lose a token.
    book_lock: Mutex<()>,
    display: DisplayName,
    /// Where apps' own names are found.
    pub names: AppNames,
}

/// Lock, and carry on with the data if another thread panicked while holding it.
fn held<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl Shared {
    pub fn new(chooser: Box<dyn Chooser>, restore: Box<dyn RestoreStore>) -> Self {
        Self {
            sessions: Mutex::new(SessionBook::new()),
            shares: Mutex::new(BTreeMap::new()),
            chooser,
            restore,
            book_lock: Mutex::new(()),
            display: DisplayName::session(),
            names: AppNames::none(),
        }
    }

    /// Open a session; `false` if the handle is already open.
    pub fn create(&self, key: &SessionKey, app_id: &str) -> bool {
        held(&self.sessions).create(key.clone(), app_id).is_some()
    }

    /// Record the app's selection; `false` if the session is not this app's.
    pub fn select(&self, key: &SessionKey, app_id: &str, selection: Selection) -> bool {
        let mut sessions = held(&self.sessions);
        let theirs = sessions
            .get(key)
            .is_some_and(|entry| entry.app_id == app_id);
        theirs && sessions.select(key, selection).is_some()
    }

    /// The session closed from outside: stop its streams.
    pub fn close(&self, key: &SessionKey) {
        held(&self.sessions).apply(key, CastIn::Close);
        held(&self.shares).remove(key);
    }

    /// A source went away by itself: close the session and tell the portal.
    pub async fn ended(&self, key: &SessionKey, connection: &Connection) {
        let known = !held(&self.sessions)
            .apply(key, CastIn::StreamEnded)
            .is_empty();
        held(&self.shares).remove(key);
        if known {
            let _ = connection
                .emit_signal(
                    None::<&str>,
                    key.0.as_str(),
                    "org.freedesktop.impl.portal.Session",
                    "Closed",
                    &(),
                )
                .await;
            forget_object(connection, key).await;
        }
    }

    /// Run a Start call to its answer.
    pub async fn start(
        self: &Arc<Self>,
        key: &SessionKey,
        app_id: &str,
        connection: &Connection,
    ) -> Outcome {
        let selection = {
            let sessions = held(&self.sessions);
            match sessions.get(key) {
                Some(entry) if entry.app_id == app_id => entry
                    .selection
                    .clone()
                    .unwrap_or_else(|| options::selection(&Dict::new())),
                _ => return Outcome::Failed,
            }
        };
        let restored = self.restored(&selection, app_id).await;
        let outcome = self
            .run_machine(key, app_id, &selection, restored, connection)
            .await;
        if !matches!(outcome, Outcome::Granted(_)) {
            held(&self.shares).remove(key);
            forget_object(connection, key).await;
        }
        outcome
    }

    async fn run_machine(
        self: &Arc<Self>,
        key: &SessionKey,
        app_id: &str,
        selection: &Selection,
        restored: Vec<CastSource>,
        connection: &Connection,
    ) -> Outcome {
        let mut cursor = selection
            .cursor_modes
            .first()
            .copied()
            .unwrap_or(transom_core::CursorMode::Hidden);
        let mut inputs = VecDeque::from([CastIn::Start { restored }]);
        let mut outcome = Outcome::Failed;
        while let Some(input) = inputs.pop_front() {
            let outs = held(&self.sessions).apply(key, input);
            for out in outs {
                match out {
                    CastOut::AskChooser => match self.ask(selection, app_id).await {
                        Ok(Some(choice)) => {
                            cursor = choice.cursor;
                            inputs.push_back(CastIn::Picked(choice.sources));
                        }
                        Ok(None) => inputs.push_back(CastIn::Picked(Vec::new())),
                        Err(()) => {
                            held(&self.sessions).apply(key, CastIn::Close);
                            return Outcome::Failed;
                        }
                    },
                    CastOut::OpenStream(sources) => {
                        match self.open(key, sources, cursor, connection).await {
                            Some(node) => inputs.push_back(CastIn::NodeReady(node)),
                            None => inputs.push_back(CastIn::StreamFailed),
                        }
                    }
                    CastOut::CloseStream => {
                        held(&self.shares).remove(key);
                    }
                    CastOut::Reply(reply) => outcome = self.outcome(key, app_id, selection, reply),
                    // A later core adds outputs this does not know; they are ignored.
                    _ => {}
                }
            }
        }
        outcome
    }

    /// Ask the person; `Err` when the question failed or the answer was not acceptable. With no
    /// shell to ask the answer is a cancel, never a share.
    async fn ask(&self, selection: &Selection, app_id: &str) -> Result<Option<Choice>, ()> {
        let request = ChoiceRequest {
            kinds: selection.kinds,
            multiple: selection.multiple,
            cursor_modes: selection.cursor_modes.clone(),
            app_label: self.names.label(app_id),
        };
        match unavailable_is_cancel(self.chooser.pick(&request).await) {
            Ok(Some(choice)) => request
                .accept(choice.sources, choice.cursor)
                .map(Some)
                .map_err(|refusal| tracing::warn!(%refusal, "the chooser's answer was refused")),
            Ok(None) => Ok(None),
            Err(error) => {
                tracing::warn!(%error, "the person could not be asked what to share");
                Err(())
            }
        }
    }

    /// Open the streams off the async runtime and keep the share; the first node, or `None`.
    async fn open(
        self: &Arc<Self>,
        key: &SessionKey,
        sources: Vec<CastSource>,
        cursor: transom_core::CursorMode,
        connection: &Connection,
    ) -> Option<PwNode> {
        let handle = tokio::runtime::Handle::current();
        let ended = {
            let (shared, key, connection) = (Arc::clone(self), key.clone(), connection.clone());
            move || {
                let (shared, key, connection) =
                    (Arc::clone(&shared), key.clone(), connection.clone());
                handle.spawn(async move { shared.ended(&key, &connection).await });
            }
        };
        let display = self.display.clone();
        let opened =
            tokio::task::spawn_blocking(move || Share::open(&display, &sources, cursor, ended))
                .await;
        match opened {
            Ok(Ok(share)) => {
                let node = share.first_node();
                held(&self.shares).insert(key.clone(), share);
                node
            }
            Ok(Err(error)) => {
                tracing::warn!(%error, "the share could not be opened");
                None
            }
            Err(error) => {
                tracing::warn!(%error, "opening the share stopped early");
                None
            }
        }
    }

    /// Turn the machine's reply into the wire answer, keeping a restore token if asked to.
    fn outcome(
        &self,
        key: &SessionKey,
        app_id: &str,
        selection: &Selection,
        reply: CastReply,
    ) -> Outcome {
        match reply {
            CastReply::Granted { sources, .. } => {
                let Some(streams) = held(&self.shares).get(key).map(Share::infos) else {
                    return Outcome::Failed;
                };
                let kept = match selection.persist {
                    PersistMode::DoNot => None,
                    _ => self.keep(app_id, sources, selection.restore.as_ref()),
                };
                let kept = kept.as_ref().map(|token| (selection.persist, token));
                match options::granted_results(&streams, kept) {
                    Ok(results) => Outcome::Granted(results),
                    Err(error) => {
                        tracing::warn!(%error, "the reply could not be built");
                        Outcome::Failed
                    }
                }
            }
            CastReply::Cancelled => Outcome::Cancelled,
            _ => Outcome::Failed,
        }
    }

    /// What a restore token stands for now: the sources it was given, if all still exist.
    async fn restored(&self, selection: &Selection, app_id: &str) -> Vec<CastSource> {
        let Some(token) = &selection.restore else {
            return Vec::new();
        };
        let display = self.display.clone();
        let live = tokio::task::spawn_blocking(move || capture::live_sources(&display))
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default();
        let sources = self.read_book().resolve(token, app_id, &live);
        if sources.iter().all(|s| selection.kinds.offers(s)) {
            sources
        } else {
            Vec::new()
        }
    }

    /// Remember the grant under a fresh token (and forget the one it replaces). `None` when no
    /// token could be made or kept.
    fn keep(
        &self,
        app_id: &str,
        sources: Vec<CastSource>,
        replaces: Option<&RestoreToken>,
    ) -> Option<RestoreToken> {
        let token = RestoreToken::mint(entropy()?);
        let grant = Grant {
            app_id: app_id.to_owned(),
            sources,
        };
        let _book = held(&self.book_lock);
        let mut book = self.restore.load().unwrap_or_default();
        if let Some(old) = replaces {
            book.forget(old);
        }
        book.remember(token.clone(), grant);
        match self.restore.save(&book) {
            Ok(()) => Some(token),
            Err(error) => {
                tracing::warn!(%error, "the restore tokens could not be saved");
                None
            }
        }
    }

    /// The saved tokens; an empty book if they cannot be read.
    fn read_book(&self) -> RestoreBook {
        let _book = held(&self.book_lock);
        self.restore.load().unwrap_or_else(|error| {
            tracing::warn!(%error, "the restore tokens could not be read");
            RestoreBook::new()
        })
    }
}

/// Take a closed session's object off the bus.
pub(super) async fn forget_object(connection: &Connection, key: &SessionKey) {
    let _ = connection
        .object_server()
        .remove::<SessionObject, _>(key.0.as_str())
        .await;
}

/// 16 bytes from the operating system's random source.
fn entropy() -> Option<[u8; 16]> {
    let mut bytes = [0_u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .ok()?;
    Some(bytes)
}

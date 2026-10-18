//! The life of a screen-cast session: what happens between the wire calls.
//!
//! [`Shared`] holds the open sessions, the running shares, the chooser and the token store. The
//! decisions are `transom_core::step`; this runs the outputs it asks for.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use transom_core::{
    CastIn, CastOut, CastReply, CastSource, Choice, ChoiceRequest, Grant, PersistMode, PwNode,
    Remembered, RestoreBook, RestoreToken,
};
use zbus::Connection;

use super::options;
use super::restore::{RestoreStore, StoreError};
use super::run::Share;
use super::session::{Refusal, Selection, SessionBook, SessionKey};
use super::{Dict, SessionObject};
use crate::app_proof::{AppProof, AppProver, NoProof};
use crate::capture::{DisplayName, Job};
use crate::chooser::{Chooser, unavailable_is_cancel};
use crate::names::AppNames;
use crate::peers::{Creator, Tether};
use crate::response::Response;

/// Everything the interface and the session objects share.
#[derive(Debug)]
pub(super) struct Shared {
    sessions: Mutex<SessionBook>,
    shares: Mutex<BTreeMap<SessionKey, Share>>,
    chooser: Box<dyn Chooser>,
    restore: Box<dyn RestoreStore>,
    /// Says which apps are what their ids say; only those are remembered.
    pub prover: Box<dyn AppProver>,
    /// Held while the token book is read, changed and written, so two Starts do not lose a token.
    book_lock: Mutex<()>,
    /// Where the pictures come from.
    pub display: DisplayName,
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
            prover: Box::new(NoProof),
            book_lock: Mutex::new(()),
            display: DisplayName::session(),
            names: AppNames::none(),
        }
    }

    /// Open a session hung on `tether`; the handles dropped to make room, or why not.
    pub fn create(
        &self,
        key: &SessionKey,
        app_id: &str,
        tether: Tether,
    ) -> Result<Vec<SessionKey>, Refusal> {
        held(&self.sessions).create(key.clone(), app_id, tether, Instant::now())
    }

    /// The peer that opened the session `key`, if it is open.
    pub fn creator(&self, key: &SessionKey) -> Option<Creator> {
        held(&self.sessions).get(key).map(|e| e.tether.creator())
    }

    /// End every session hung on `peer`, stopping its streams; the handles ended.
    pub fn close_for_peer(&self, peer: &str) -> Vec<SessionKey> {
        let keys = held(&self.sessions).held_by(peer);
        for key in &keys {
            self.close(key);
        }
        keys
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
    ) -> Response {
        let selection = {
            let sessions = held(&self.sessions);
            match sessions.get(key) {
                Some(entry) if entry.app_id == app_id => entry
                    .selection
                    .clone()
                    .unwrap_or_else(|| options::selection(&Dict::new())),
                _ => return Response::Failed,
            }
        };
        // One job for the whole start: what the restore token is checked against and every stream
        // opened afterwards use one connection to the compositor, asked for once.
        let job = Arc::new(Job::new(&self.display));
        let restored = self.restored(&selection, key, app_id, &job).await;
        let outcome = self
            .run_machine(key, app_id, &selection, restored, &job, connection)
            .await;
        if !matches!(outcome, Response::Granted(_)) {
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
        job: &Arc<Job>,
        connection: &Connection,
    ) -> Response {
        let mut cursor = selection
            .cursor_modes
            .first()
            .copied()
            .unwrap_or(transom_core::CursorMode::Hidden);
        let mut inputs = VecDeque::from([CastIn::Start { restored }]);
        let mut outcome = Response::Failed;
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
                            return Response::Failed;
                        }
                    },
                    CastOut::OpenStream(sources) => {
                        match self.open(key, sources, cursor, job, connection).await {
                            Some(node) => inputs.push_back(CastIn::NodeReady(node)),
                            None => inputs.push_back(CastIn::StreamFailed),
                        }
                    }
                    CastOut::CloseStream => {
                        held(&self.shares).remove(key);
                    }
                    CastOut::Reply(reply) => {
                        outcome = self.outcome(key, app_id, selection, reply).await;
                    }
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
        job: &Arc<Job>,
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
        let job = Arc::clone(job);
        let opened =
            tokio::task::spawn_blocking(move || Share::open(&job, &sources, cursor, ended)).await;
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
    async fn outcome(
        self: &Arc<Self>,
        key: &SessionKey,
        app_id: &str,
        selection: &Selection,
        reply: CastReply,
    ) -> Response {
        match reply {
            CastReply::Granted { sources, .. } => {
                let Some(streams) = held(&self.shares).get(key).map(Share::infos) else {
                    return Response::Failed;
                };
                let kept = match selection.persist {
                    PersistMode::DoNot => None,
                    _ if !self.remembers(key, app_id).await => None,
                    mode => {
                        self.keep(app_id, sources, mode, selection.restore.clone())
                            .await
                    }
                };
                let kept = kept.as_ref().map(|token| (selection.persist, token));
                match options::granted_results(&streams, kept) {
                    Ok(results) => Response::Granted(results),
                    Err(error) => {
                        tracing::warn!(%error, "the reply could not be built");
                        Response::Failed
                    }
                }
            }
            CastReply::Cancelled => Response::Cancelled,
            _ => Response::Failed,
        }
    }

    /// Whether `app_id`, calling through `key`, is shown to be what its id says. Only such an app
    /// is remembered or restored; any other is asked every time (NEW-PI-2).
    async fn remembers(&self, key: &SessionKey, app_id: &str) -> bool {
        match self.prover.prove(&key.0, app_id).await {
            AppProof::Proven => true,
            AppProof::Unproven(why) => {
                tracing::info!(%why, "this app is asked every time: it is not shown to be what its id says");
                false
            }
        }
    }

    /// What a restore token stands for now: the sources it was given, if all still exist and the
    /// grant has not expired. An app with no identity is never restored.
    async fn restored(
        self: &Arc<Self>,
        selection: &Selection,
        key: &SessionKey,
        app_id: &str,
        job: &Arc<Job>,
    ) -> Vec<CastSource> {
        let Some(token) = selection.restore.clone() else {
            return Vec::new();
        };
        if app_id.is_empty() || !self.remembers(key, app_id).await {
            return Vec::new();
        }
        let job = Arc::clone(job);
        let live = tokio::task::spawn_blocking(move || job.live_sources())
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default();
        let (this, app_id) = (Arc::clone(self), app_id.to_owned());
        let sources = tokio::task::spawn_blocking(move || {
            this.read_book().resolve(&token, &app_id, &live, unix_now())
        })
        .await
        .unwrap_or_default();
        if sources.iter().all(|s| selection.kinds.offers(s)) {
            sources
        } else {
            Vec::new()
        }
    }

    /// Remember the grant under a fresh token (and forget the one it replaces), off the async
    /// runtime because the book is a file. `None` when nothing may be kept (the app has no
    /// identity), or no token could be made or kept; the person is then asked again next time.
    async fn keep(
        self: &Arc<Self>,
        app_id: &str,
        sources: Vec<CastSource>,
        mode: PersistMode,
        replaces: Option<RestoreToken>,
    ) -> Option<RestoreToken> {
        let (this, app_id) = (Arc::clone(self), app_id.to_owned());
        tokio::task::spawn_blocking(move || {
            this.keep_now(&app_id, sources, mode, replaces.as_ref())
        })
        .await
        .ok()
        .flatten()
    }

    fn keep_now(
        &self,
        app_id: &str,
        sources: Vec<CastSource>,
        mode: PersistMode,
        replaces: Option<&RestoreToken>,
    ) -> Option<RestoreToken> {
        let now = unix_now();
        let Some(grant) = Grant::issue(app_id, sources, mode, now) else {
            tracing::info!(
                "this share is not remembered: the app has no identity to remember it by"
            );
            return None;
        };
        let token = RestoreToken::mint(crate::random::bytes::<16>()?);
        let _book = held(&self.book_lock);
        let mut book = match self.restore.load() {
            Ok(book) => book,
            // The damaged file was set aside by the store, so nothing is lost by starting anew.
            Err(StoreError::Damaged) => {
                tracing::error!("the saved grants were damaged and were set aside; starting anew");
                RestoreBook::new()
            }
            // Contents unknown: saving now could erase everyone else's grants.
            Err(error) => {
                tracing::error!(%error, "the saved grants could not be read, so this one is not saved");
                return None;
            }
        };
        if let Some(old) = replaces {
            book.forget(old);
        }
        book.remember(token.clone(), grant);
        book.prune(now);
        match self.restore.save(&book) {
            Ok(()) => Some(token),
            Err(error) => {
                tracing::warn!(%error, "the restore tokens could not be saved");
                None
            }
        }
    }

    /// The apps the person has let share without asking again, off the async runtime.
    pub async fn remembered(self: &Arc<Self>) -> Vec<Remembered> {
        let this = Arc::clone(self);
        tokio::task::spawn_blocking(move || this.read_book().remembered(unix_now()))
            .await
            .unwrap_or_default()
    }

    /// Take back everything remembered for `app_id`; how many grants went, or `None` when the
    /// saved book could not be read or written (nothing is then changed).
    pub async fn revoke(self: &Arc<Self>, app_id: &str) -> Option<usize> {
        let (this, app_id) = (Arc::clone(self), app_id.to_owned());
        tokio::task::spawn_blocking(move || this.revoke_now(&app_id))
            .await
            .ok()
            .flatten()
    }

    fn revoke_now(&self, app_id: &str) -> Option<usize> {
        let _book = held(&self.book_lock);
        let mut book = match self.restore.load() {
            Ok(book) => book,
            // Nothing readable is nothing remembered; the store set a damaged file aside.
            Err(StoreError::Damaged) => return Some(0),
            Err(error) => {
                tracing::error!(%error, "the saved grants could not be read, so none was taken back");
                return None;
            }
        };
        let gone = book.forget_app(app_id);
        if gone == 0 {
            return Some(0);
        }
        match self.restore.save(&book) {
            Ok(()) => Some(gone),
            Err(error) => {
                tracing::error!(%error, "the saved grants could not be written, so none was taken back");
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

/// Seconds since the Unix epoch, the clock grants are dated by.
fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

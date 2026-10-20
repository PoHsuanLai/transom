//! The life of a screen-cast session: what happens between the wire calls.
//!
//! [`Shared`] holds the open sessions, the running shares, the chooser and the token store. The
//! decisions are `transom_core::step`; this runs the outputs it asks for.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use transom_core::{
    CastIn, CastOut, CastReply, CastSource, Choice, ChoiceRequest, PersistMode, PwNode, Remembered,
};
use zbus::Connection;

use super::options;
use super::restore::RestoreStore;
use super::run::Share;
use super::session::{Refusal, Selection, SessionBook, SessionKey};
use super::{Dict, SessionObject};
use crate::app_proof::{AppProver, NoProof, Who};
use crate::capture::{DisplayName, Job};
use crate::chooser::{Chooser, unavailable_is_cancel};
use crate::keeper::{Keeper, Kept, unix_now};
use crate::names::AppNames;
use crate::peers::{Creator, Tether};
use crate::response::Response;

/// Everything the interface and the session objects share.
#[derive(Debug)]
pub(super) struct Shared {
    sessions: Mutex<SessionBook>,
    shares: Mutex<BTreeMap<SessionKey, Share>>,
    chooser: Box<dyn Chooser>,
    /// The book of what is remembered, shared with the remote-control interface.
    pub keeper: Arc<Keeper>,
    /// Says which apps are what their ids say; only those are remembered.
    pub prover: Arc<dyn AppProver>,
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

/// What the person said to "what do you want to share".
enum Asked {
    /// They chose these sources.
    Picked(Choice),
    /// They cancelled, or there was no one to ask.
    Cancelled,
    /// The question failed or the answer was not acceptable.
    Failed,
}

impl Shared {
    pub fn new(chooser: Box<dyn Chooser>, restore: Box<dyn RestoreStore>) -> Self {
        Self {
            sessions: Mutex::new(SessionBook::new()),
            shares: Mutex::new(BTreeMap::new()),
            chooser,
            keeper: Arc::new(Keeper::new(restore)),
            prover: Arc::new(NoProof),
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
        let who = Who::identify(self.prover.as_ref(), &key.0, app_id).await;
        let restored = self.restored(&selection, &who, &job).await;
        let outcome = self
            .run_machine(key, &who, &selection, restored, &job, connection)
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
        who: &Who,
        selection: &Selection,
        restored: Vec<CastSource>,
        job: &Arc<Job>,
        connection: &Connection,
    ) -> Response {
        let mut cursor = selection
            .offer
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
                    CastOut::AskChooser => match self.ask(selection, who).await {
                        Asked::Picked(choice) => {
                            cursor = choice.cursor;
                            inputs.push_back(CastIn::Picked(choice.sources));
                        }
                        Asked::Cancelled => inputs.push_back(CastIn::Picked(Vec::new())),
                        Asked::Failed => {
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
                        outcome = self.outcome(key, who, selection, reply).await;
                    }
                    // A later core adds outputs this does not know; they are ignored.
                    _ => {}
                }
            }
        }
        outcome
    }

    /// Ask the person; `Failed` when the question failed or the answer was not acceptable. With no
    /// shell to ask the answer is a cancel, never a share.
    async fn ask(&self, selection: &Selection, who: &Who) -> Asked {
        let request = ChoiceRequest {
            offer: selection.offer.clone(),
            app_label: self.names.label(&who.shown),
            // Only a proven app's yes is kept (see `outcome`), so only its picker says so.
            remembered: match who.proven {
                Some(_) => selection.persist,
                None => PersistMode::DoNot,
            },
        };
        match unavailable_is_cancel(self.chooser.pick(&request).await) {
            Ok(Some(choice)) => match request.accept(choice.sources, choice.cursor) {
                Ok(accepted) => Asked::Picked(accepted),
                Err(refusal) => {
                    tracing::warn!(%refusal, "the chooser's answer was refused");
                    Asked::Failed
                }
            },
            Ok(None) => Asked::Cancelled,
            Err(error) => {
                tracing::warn!(%error, "the person could not be asked what to share");
                Asked::Failed
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
        who: &Who,
        selection: &Selection,
        reply: CastReply,
    ) -> Response {
        match reply {
            CastReply::Granted { sources, .. } => {
                let Some(streams) = held(&self.shares).get(key).map(Share::infos) else {
                    return Response::Failed;
                };
                // Only an app shown to be what it says is remembered, by the id it is shown by.
                let kept = match (selection.persist, who.proven.as_deref()) {
                    (PersistMode::Keep(keep), Some(app_id)) => self
                        .keeper
                        .keep(
                            app_id,
                            Kept::Screen(sources),
                            keep,
                            selection.restore.clone(),
                        )
                        .await
                        .map(|token| (keep, token)),
                    _ => None,
                };
                let kept = kept.as_ref().map(|(keep, token)| (*keep, token));
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

    /// What a restore token stands for now: the sources it was given, if all still exist and the
    /// grant has not expired. An app that is not shown to be what it says is never restored.
    async fn restored(&self, selection: &Selection, who: &Who, job: &Arc<Job>) -> Vec<CastSource> {
        let (Some(token), Some(app_id)) = (selection.restore.as_ref(), who.proven.as_deref())
        else {
            return Vec::new();
        };
        let job = Arc::clone(job);
        let live = tokio::task::spawn_blocking(move || job.live_sources())
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or_default();
        let sources = self
            .keeper
            .book()
            .await
            .resolve(token, app_id, &live, unix_now());
        if sources.iter().all(|s| selection.offer.kinds.offers(s)) {
            sources
        } else {
            Vec::new()
        }
    }

    /// The apps the person has let share without asking again, off the async runtime.
    pub async fn remembered(self: &Arc<Self>) -> Vec<Remembered> {
        self.keeper.remembered().await
    }

    /// Take back everything remembered for `app_id`; how many grants went, or `None` when the
    /// saved book could not be read or written (nothing is then changed).
    pub async fn revoke(self: &Arc<Self>, app_id: &str) -> Option<usize> {
        self.keeper.revoke(app_id).await
    }
}

/// Take a closed session's object off the bus.
pub(super) async fn forget_object(connection: &Connection, key: &SessionKey) {
    let _ = connection
        .object_server()
        .remove::<SessionObject, _>(key.0.as_str())
        .await;
}

//! The state of the open sessions and the rules that move it: asking once, one socket, and
//! taking the socket back when the session ends.

use std::os::fd::OwnedFd;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use super::book::{Book, Entry, Socket, Stage};
use super::devices::DeviceKinds;
use super::eis::{EisError, EisHandout};
use crate::peers::{Creator, Tether};
use crate::screencast::session::{Refusal, SessionKey};

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
    /// The session already has its one input socket.
    #[error("this session already has its input connection")]
    AlreadyConnected,
    /// The compositor would not hand out a connection.
    #[error(transparent)]
    Eis(#[from] EisError),
}

/// What the person is about to be asked about.
#[derive(Debug)]
pub(super) struct Asking {
    pub id: u64,
    pub app_id: String,
    pub kinds: DeviceKinds,
}

/// What came of asking.
#[derive(Clone, Copy, Debug)]
pub(super) enum Verdict {
    /// They allowed it.
    Allow,
    /// They declined: the session ends.
    Decline,
    /// The question could not be asked: the session may be asked again.
    Retract,
}

/// Whether a change reached the session it was meant for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Settled {
    /// It did.
    Applied,
    /// The session was closed meanwhile (or closed and opened again): nothing changed.
    Gone,
}

/// The open sessions and the compositor that gives them input sockets.
#[derive(Debug)]
pub(super) struct Core {
    book: Mutex<Book>,
    eis: Box<dyn EisHandout>,
}

impl Core {
    pub fn new(eis: Box<dyn EisHandout>) -> Self {
        Self {
            book: Mutex::new(Book::starting_after(clock_nanos())),
            eis,
        }
    }

    /// Tell the compositor to drop every input socket an earlier run of this portal handed out.
    pub async fn end_all(&self) {
        self.eis.end_all().await;
    }

    fn lock(&self) -> MutexGuard<'_, Book> {
        self.book.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Opens `key` for `app_id`, hung on `tether`; the handles dropped to make room, or why not.
    pub fn create(
        &self,
        key: SessionKey,
        app_id: &str,
        tether: Tether,
    ) -> Result<Vec<SessionKey>, Refusal> {
        self.lock().open(key, app_id, tether, Instant::now())
    }

    /// The peer that opened the session `key`, if it is open.
    pub fn creator(&self, key: &SessionKey) -> Option<Creator> {
        self.lock().entries.get(key).map(|e| e.tether.creator())
    }

    /// Ends every session hung on `peer`, taking back the sockets handed out; the handles ended.
    pub async fn peer_gone(&self, peer: &str) -> Vec<SessionKey> {
        let gone = self.lock().remove_held_by(peer);
        let mut keys = Vec::with_capacity(gone.len());
        for (key, entry) in gone {
            self.revoke(Some(entry)).await;
            keys.push(key);
        }
        keys
    }

    /// Records the kinds the app wants. Only before `Start`.
    pub fn select(&self, key: &SessionKey, kinds: DeviceKinds) -> Settled {
        match self.lock().entries.get_mut(key) {
            Some(entry) if entry.stage == Stage::Created => {
                entry.kinds = kinds;
                Settled::Applied
            }
            _ => Settled::Gone,
        }
    }

    /// Marks the session as being asked about and says what to ask. `None` if it is unknown,
    /// already asked about or allowed, or wants no device at all.
    pub fn start_asking(&self, key: &SessionKey) -> Option<Asking> {
        let mut book = self.lock();
        let entry = book.entries.get_mut(key)?;
        if entry.stage != Stage::Created || entry.kinds.is_empty() {
            return None;
        }
        entry.stage = Stage::Asking;
        Some(Asking {
            id: entry.id,
            app_id: entry.app_id.clone(),
            kinds: entry.kinds,
        })
    }

    /// Applies the answer to the entry number `id` under `key`, and only to that one: the handle
    /// may have been closed and opened again while the person decided, and the new session must
    /// not inherit a yes given to the old.
    pub fn settle(&self, key: &SessionKey, id: u64, verdict: Verdict) -> Settled {
        let mut book = self.lock();
        let Some(entry) = book.entry_mut(key, id).filter(|e| e.stage == Stage::Asking) else {
            return Settled::Gone;
        };
        match verdict {
            Verdict::Allow => {
                entry.stage = Stage::Started {
                    socket: Socket::NotYet,
                }
            }
            Verdict::Retract => entry.stage = Stage::Created,
            Verdict::Decline => {
                book.entries.remove(key);
            }
        }
        Settled::Applied
    }

    /// The one socket a session may have, once the person has allowed it.
    pub async fn connect(&self, key: &SessionKey) -> Result<OwnedFd, ConnectError> {
        let (id, kinds) = {
            let mut book = self.lock();
            let entry = book.entries.get_mut(key).ok_or(ConnectError::NoSession)?;
            let Stage::Started { socket } = &mut entry.stage else {
                return Err(ConnectError::NotAllowed);
            };
            if *socket == Socket::Handed {
                return Err(ConnectError::AlreadyConnected);
            }
            // Marked before the call so two calls at once cannot both get one.
            *socket = Socket::Handed;
            (entry.id, entry.kinds)
        };
        let handed = self.eis.connect(id, kinds).await;
        let still_open = self.lock().entry_mut(key, id).is_some();
        match handed {
            Ok(fd) if still_open => Ok(fd),
            Ok(fd) => {
                // Closed while the compositor was answering: take the socket straight back.
                drop(fd);
                self.eis.end(id).await;
                Err(ConnectError::NoSession)
            }
            Err(error) => {
                if let Some(entry) = self.lock().entry_mut(key, id)
                    && let Stage::Started { socket } = &mut entry.stage
                {
                    *socket = Socket::NotYet;
                }
                Err(error.into())
            }
        }
    }

    /// Ends the session `key` (an unknown one is ignored) and takes back its input socket.
    pub async fn close(&self, key: &SessionKey) {
        let gone = self.lock().entries.remove(key);
        self.revoke(gone).await;
    }

    /// Takes back the socket of a session that has just been removed from the book.
    pub async fn revoke(&self, gone: Option<Entry>) {
        if let Some(entry) = gone.filter(|entry| {
            matches!(
                entry.stage,
                Stage::Started {
                    socket: Socket::Handed
                }
            )
        }) {
            self.eis.end(entry.id).await;
        }
    }

    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }
}

/// Nanoseconds since the Unix epoch, the number a run's sessions count up from (see
/// [`Book::starting_after`]); a clock before the epoch counts as zero.
fn clock_nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| {
            u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX)
        })
}

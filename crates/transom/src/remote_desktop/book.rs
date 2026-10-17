//! The sessions of the remote-control portal, and what has happened in each.

use std::collections::BTreeMap;
use std::time::Instant;

use super::devices::DeviceKinds;
use crate::peers::{Admission, SESSION_LIMITS, Tether, UNSTARTED_TTL, admit};
use crate::screencast::session::{Refusal, SessionKey};

/// How far a session has come.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stage {
    /// Created; the app is choosing devices.
    Created,
    /// The person is being asked. A second `Start` in this stage is refused, so one session never
    /// shows two questions.
    Asking,
    /// The person allowed it.
    Started,
}

/// Whether the session has taken its input socket. A session takes at most one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Socket {
    /// None handed out.
    NotYet,
    /// One handed out (or being handed out).
    Handed,
}

/// One session.
#[derive(Clone, Debug)]
pub(super) struct Entry {
    /// The number the compositor tags this session's input with.
    pub id: u64,
    pub app_id: String,
    pub kinds: DeviceKinds,
    pub stage: Stage,
    pub socket: Socket,
    /// The peers the session hangs on: when one leaves the bus the session ends.
    pub tether: Tether,
    /// When it was opened.
    pub opened: Instant,
}

/// Every open session.
#[derive(Debug)]
pub(super) struct Book {
    next: u64,
    pub entries: BTreeMap<SessionKey, Entry>,
}

impl Book {
    /// An empty book whose first session is numbered `after + 1`. The compositor keys what it
    /// handed out by these numbers and may outlive this process, so a new run starts above every
    /// number an earlier one could have used (the clock, in nanoseconds) and an end called for
    /// number 1 can never reach another run's session.
    pub fn starting_after(after: u64) -> Self {
        Self {
            next: after,
            entries: BTreeMap::new(),
        }
    }

    /// A new entry for `app_id` hung on `tether` at time `now`; its number is never reused. A
    /// full book drops the sessions that never began and are older than [`UNSTARTED_TTL`] to make
    /// room; the result is the handles dropped.
    pub fn open(
        &mut self,
        key: SessionKey,
        app_id: &str,
        tether: Tether,
        now: Instant,
    ) -> Result<Vec<SessionKey>, Refusal> {
        if self.entries.contains_key(&key) {
            return Err(Refusal::AlreadyOpen);
        }
        let mut dropped = Vec::new();
        if self.admits(&tether) == Admission::Full {
            dropped = self.expire(now);
            if self.admits(&tether) == Admission::Full {
                return Err(Refusal::Full);
            }
        }
        self.next += 1;
        self.entries.insert(
            key,
            Entry {
                id: self.next,
                app_id: app_id.to_owned(),
                kinds: DeviceKinds::ALL,
                stage: Stage::Created,
                socket: Socket::NotYet,
                tether,
                opened: now,
            },
        );
        Ok(dropped)
    }

    fn admits(&self, tether: &Tether) -> Admission {
        admit(
            self.entries.values().map(|e| &e.tether),
            tether,
            SESSION_LIMITS,
        )
    }

    /// Drop the sessions that never began and are older than [`UNSTARTED_TTL`] at `now`.
    fn expire(&mut self, now: Instant) -> Vec<SessionKey> {
        let stale: Vec<SessionKey> = self
            .entries
            .iter()
            .filter(|(_, e)| {
                e.stage == Stage::Created && now.saturating_duration_since(e.opened) > UNSTARTED_TTL
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in &stale {
            self.entries.remove(key);
        }
        stale
    }

    /// Remove every session hung on `peer`; the handles and what was removed.
    pub fn remove_held_by(&mut self, peer: &str) -> Vec<(SessionKey, Entry)> {
        let keys: Vec<SessionKey> = self
            .entries
            .iter()
            .filter(|(_, e)| e.tether.holds(peer))
            .map(|(key, _)| key.clone())
            .collect();
        keys.into_iter()
            .filter_map(|key| self.entries.remove(&key).map(|entry| (key, entry)))
            .collect()
    }

    /// The entry under `key` if it is still entry number `id` (the handle may have been closed
    /// and opened again while something waited).
    pub fn entry_mut(&mut self, key: &SessionKey, id: u64) -> Option<&mut Entry> {
        self.entries.get_mut(key).filter(|entry| entry.id == id)
    }
}

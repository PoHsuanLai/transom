//! The table both session portals keep their open sessions in, with the limits on it.
//!
//! Screen-cast and remote-desktop sessions are admitted, expired and torn down the same way: a
//! handle opens once, a caller and the whole table are capped (a full table first drops the
//! sessions that never began and have waited too long), and everything hung on a peer that left
//! the bus goes. These are security limits, so there is one copy of them.

use std::collections::BTreeMap;
use std::time::Instant;

use crate::peers::{Admission, SESSION_LIMITS, Tether, UNSTARTED_TTL, admit};

/// The object path of a session, as the frontend gave it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionKey(pub String);

/// Why a session was not opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// The handle is open already.
    AlreadyOpen,
    /// The caller or the table is at its limit, and nothing old enough could be dropped.
    Full,
}

/// What the table needs to know about one session.
pub trait Slot {
    /// The peers the session hangs on: when one leaves the bus the session ends.
    fn tether(&self) -> &Tether;
    /// When it was opened.
    fn opened(&self) -> Instant;
    /// Whether it has not begun yet (only such a session is dropped to make room).
    fn unstarted(&self) -> bool;
}

/// Every open session of one portal, by handle.
#[derive(Debug)]
pub struct Table<E> {
    /// The sessions.
    pub(crate) entries: BTreeMap<SessionKey, E>,
}

impl<E> Default for Table<E> {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }
}

impl<E: Slot> Table<E> {
    /// Open a session under `key`, hung on `tether`, at time `now`; `make` builds it once it is
    /// admitted. A handle that is already open is left alone. When the table is full, sessions
    /// that never began and have waited longer than [`UNSTARTED_TTL`] are dropped to make room;
    /// the result is the handles dropped, whose session objects the caller takes off the bus.
    pub fn create(
        &mut self,
        key: SessionKey,
        tether: Tether,
        now: Instant,
        make: impl FnOnce(Tether) -> E,
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
        self.entries.insert(key, make(tether));
        Ok(dropped)
    }

    fn admits(&self, tether: &Tether) -> Admission {
        admit(
            self.entries.values().map(Slot::tether),
            tether,
            SESSION_LIMITS,
        )
    }

    /// Drop the sessions that never began and are older than [`UNSTARTED_TTL`] at `now`.
    pub fn expire(&mut self, now: Instant) -> Vec<SessionKey> {
        let stale: Vec<SessionKey> = self
            .entries
            .iter()
            .filter(|(_, e)| {
                e.unstarted() && now.saturating_duration_since(e.opened()) > UNSTARTED_TTL
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in &stale {
            self.entries.remove(key);
        }
        stale
    }

    /// The handles of the sessions hung on `peer` (a unique name).
    pub fn held_by(&self, peer: &str) -> Vec<SessionKey> {
        self.entries
            .iter()
            .filter(|(_, e)| e.tether().holds(peer))
            .map(|(key, _)| key.clone())
            .collect()
    }

    /// Remove every session hung on `peer`; the handles and what was removed.
    pub fn remove_held_by(&mut self, peer: &str) -> Vec<(SessionKey, E)> {
        self.held_by(peer)
            .into_iter()
            .filter_map(|key| self.entries.remove(&key).map(|entry| (key, entry)))
            .collect()
    }
}

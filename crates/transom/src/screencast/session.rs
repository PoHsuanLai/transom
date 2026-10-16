//! The portal's open screen-cast sessions: one record per session handle.

use std::collections::BTreeMap;
use std::time::Instant;

use transom_core::{
    CastIn, CastOut, CastSession, CursorMode, PersistMode, RestoreToken,
    choice::{Multiple, SourceKinds},
    step,
};

use crate::peers::{Admission, SESSION_LIMITS, Tether, UNSTARTED_TTL, admit};

/// The object path of a session, as the frontend gave it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionKey(pub String);

/// What the app said at `SelectSources`, kept until `Start`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    /// The kinds it asked for.
    pub kinds: SourceKinds,
    /// One source or several.
    pub multiple: Multiple,
    /// The cursor modes it allows; the first is its preference.
    pub cursor_modes: Vec<CursorMode>,
    /// Whether to hand back a restore token.
    pub persist: PersistMode,
    /// The restore token the app brought, if it brought one of ours.
    pub restore: Option<RestoreToken>,
}

/// One session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The app that made it.
    pub app_id: String,
    /// Where the state machine stands.
    pub state: CastSession,
    /// What the app selected, once it has.
    pub selection: Option<Selection>,
    /// The peers the session hangs on: when one leaves the bus the session ends.
    pub tether: Tether,
    /// When it was opened.
    pub opened: Instant,
}

/// Why a session was not opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// The handle is open already.
    AlreadyOpen,
    /// The caller or the book is at its limit, and nothing old enough could be dropped.
    Full,
}

/// Every open session.
#[derive(Debug, Default)]
pub struct SessionBook {
    entries: BTreeMap<SessionKey, Entry>,
}

impl SessionBook {
    /// An empty book.
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a session for `app_id`, hung on `tether`, at time `now`. A handle that is already
    /// open is left alone. When the book is full, sessions that never began and have waited
    /// longer than [`UNSTARTED_TTL`] are dropped to make room; the result is the handles
    /// dropped, whose session objects the caller takes off the bus.
    pub fn create(
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
        self.entries.insert(
            key,
            Entry {
                app_id: app_id.to_owned(),
                state: CastSession::Created,
                selection: None,
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
    pub fn expire(&mut self, now: Instant) -> Vec<SessionKey> {
        let stale: Vec<SessionKey> = self
            .entries
            .iter()
            .filter(|(_, e)| {
                e.state == CastSession::Created
                    && now.saturating_duration_since(e.opened) > UNSTARTED_TTL
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in &stale {
            self.entries.remove(key);
        }
        stale
    }

    /// The sessions hung on `peer` (a unique name).
    pub fn held_by(&self, peer: &str) -> Vec<SessionKey> {
        self.entries
            .iter()
            .filter(|(_, e)| e.tether.holds(peer))
            .map(|(key, _)| key.clone())
            .collect()
    }

    /// The session under `key`.
    pub fn get(&self, key: &SessionKey) -> Option<&Entry> {
        self.entries.get(key)
    }

    /// Record what the app selected. `None` if there is no such session.
    pub fn select(&mut self, key: &SessionKey, selection: Selection) -> Option<()> {
        self.entries.get_mut(key)?.selection = Some(selection);
        Some(())
    }

    /// Feed one input to a session and return what the caller must do. A closed session is
    /// dropped from the book. An unknown key yields nothing.
    pub fn apply(&mut self, key: &SessionKey, input: CastIn) -> Vec<CastOut> {
        let Some(entry) = self.entries.get_mut(key) else {
            return Vec::new();
        };
        let current = std::mem::replace(&mut entry.state, CastSession::Closed);
        let (next, outs) = step(current, input);
        entry.state = next;
        if entry.state == CastSession::Closed {
            self.entries.remove(key);
        }
        outs
    }

    /// How many sessions are open.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether none are open.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

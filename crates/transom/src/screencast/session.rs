//! The portal's open screen-cast sessions: one record per session handle.

use std::time::Instant;

use transom_core::{CastIn, CastOut, CastSession, Offer, PersistMode, RestoreToken, step};

use crate::peers::Tether;
pub use crate::table::{Refusal, SessionKey};
use crate::table::{Slot, Table};

/// What the app said at `SelectSources`, kept until `Start`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    /// The kinds it asked for, one source or several, and the cursor modes it allows (the first
    /// is its preference).
    pub offer: Offer,
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

impl Slot for Entry {
    fn tether(&self) -> &Tether {
        &self.tether
    }

    fn opened(&self) -> Instant {
        self.opened
    }

    fn unstarted(&self) -> bool {
        self.state == CastSession::Created
    }
}

/// Every open session.
#[derive(Debug, Default)]
pub struct SessionBook {
    table: Table<Entry>,
}

impl SessionBook {
    /// An empty book.
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a session for `app_id`, hung on `tether`, at time `now`. A handle that is already
    /// open is left alone. When the book is full, sessions that never began and have waited
    /// longer than [`crate::peers::UNSTARTED_TTL`] are dropped to make room; the result is the
    /// handles dropped, whose session objects the caller takes off the bus.
    pub fn create(
        &mut self,
        key: SessionKey,
        app_id: &str,
        tether: Tether,
        now: Instant,
    ) -> Result<Vec<SessionKey>, Refusal> {
        self.table.create(key, tether, now, |tether| Entry {
            app_id: app_id.to_owned(),
            state: CastSession::Created,
            selection: None,
            tether,
            opened: now,
        })
    }

    /// Drop the sessions that never began and are older than [`crate::peers::UNSTARTED_TTL`] at
    /// `now`.
    pub fn expire(&mut self, now: Instant) -> Vec<SessionKey> {
        self.table.expire(now)
    }

    /// The sessions hung on `peer` (a unique name).
    pub fn held_by(&self, peer: &str) -> Vec<SessionKey> {
        self.table.held_by(peer)
    }

    /// The session under `key`.
    pub fn get(&self, key: &SessionKey) -> Option<&Entry> {
        self.table.entries.get(key)
    }

    /// Record what the app selected. `None` if there is no such session.
    pub fn select(&mut self, key: &SessionKey, selection: Selection) -> Option<()> {
        self.table.entries.get_mut(key)?.selection = Some(selection);
        Some(())
    }

    /// Feed one input to a session and return what the caller must do. A closed session is
    /// dropped from the book. An unknown key yields nothing.
    pub fn apply(&mut self, key: &SessionKey, input: CastIn) -> Vec<CastOut> {
        let Some(entry) = self.table.entries.get_mut(key) else {
            return Vec::new();
        };
        let current = std::mem::replace(&mut entry.state, CastSession::Closed);
        let (next, outs) = step(current, input);
        entry.state = next;
        if entry.state == CastSession::Closed {
            self.table.entries.remove(key);
        }
        outs
    }

    /// How many sessions are open.
    pub fn len(&self) -> usize {
        self.table.entries.len()
    }

    /// Whether none are open.
    pub fn is_empty(&self) -> bool {
        self.table.entries.is_empty()
    }
}

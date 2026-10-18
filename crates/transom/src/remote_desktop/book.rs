//! The sessions of the remote-control portal, and what has happened in each.

use std::time::Instant;

use super::devices::DeviceKinds;
use crate::peers::Tether;
use crate::table::{Refusal, SessionKey, Slot, Table};

/// How far a session has come.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Stage {
    /// Created; the app is choosing devices.
    Created,
    /// The person is being asked. A second `Start` in this stage is refused, so one session never
    /// shows two questions.
    Asking,
    /// The person allowed it; `socket` says whether the session has taken its input socket.
    Started { socket: Socket },
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
        self.stage == Stage::Created
    }
}

/// Every open session.
#[derive(Debug)]
pub(super) struct Book {
    next: u64,
    pub table: Table<Entry>,
}

impl Book {
    /// An empty book whose first session is numbered `after + 1`. The compositor keys what it
    /// handed out by these numbers and may outlive this process, so a new run starts above every
    /// number an earlier one could have used (the clock, in nanoseconds) and an end called for
    /// number 1 can never reach another run's session.
    pub fn starting_after(after: u64) -> Self {
        Self {
            next: after,
            table: Table::default(),
        }
    }

    /// A new entry for `app_id` hung on `tether` at time `now`; its number is never reused. A
    /// full book drops the sessions that never began and are older than [`crate::peers::UNSTARTED_TTL`] to make
    /// room; the result is the handles dropped.
    pub fn open(
        &mut self,
        key: SessionKey,
        app_id: &str,
        tether: Tether,
        now: Instant,
    ) -> Result<Vec<SessionKey>, Refusal> {
        let next = &mut self.next;
        self.table.create(key, tether, now, |tether| {
            *next += 1;
            Entry {
                id: *next,
                app_id: app_id.to_owned(),
                kinds: DeviceKinds::ALL,
                stage: Stage::Created,
                tether,
                opened: now,
            }
        })
    }

    /// The entry under `key` if it is still entry number `id` (the handle may have been closed
    /// and opened again while something waited).
    pub fn entry_mut(&mut self, key: &SessionKey, id: u64) -> Option<&mut Entry> {
        self.table
            .entries
            .get_mut(key)
            .filter(|entry| entry.id == id)
    }
}

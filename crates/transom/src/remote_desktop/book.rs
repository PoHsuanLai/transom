//! The sessions of the remote-control portal, and what has happened in each.

use std::collections::BTreeMap;

use super::devices::DeviceKinds;
use crate::screencast::session::SessionKey;

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
}

/// Every open session.
#[derive(Debug, Default)]
pub(super) struct Book {
    next: u64,
    pub entries: BTreeMap<SessionKey, Entry>,
}

impl Book {
    /// A new entry for `app_id`; its number is never reused. `None` if the handle is open.
    pub fn open(&mut self, key: SessionKey, app_id: &str) -> Option<u64> {
        if self.entries.contains_key(&key) {
            return None;
        }
        self.next += 1;
        let id = self.next;
        self.entries.insert(
            key,
            Entry {
                id,
                app_id: app_id.to_owned(),
                kinds: DeviceKinds::ALL,
                stage: Stage::Created,
                socket: Socket::NotYet,
            },
        );
        Some(id)
    }

    /// The entry under `key` if it is still entry number `id` (the handle may have been closed
    /// and opened again while something waited).
    pub fn entry_mut(&mut self, key: &SessionKey, id: u64) -> Option<&mut Entry> {
        self.entries.get_mut(key).filter(|entry| entry.id == id)
    }
}

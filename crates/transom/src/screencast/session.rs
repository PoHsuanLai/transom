//! The portal's open screen-cast sessions: one record per session handle.

use std::collections::BTreeMap;

use transom_core::{
    CastIn, CastOut, CastSession, CursorMode, PersistMode,
    choice::{Multiple, SourceKinds},
    step,
};

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

    /// Open a session for `app_id`. A handle that is already open is left alone and `None` is
    /// returned.
    pub fn create(&mut self, key: SessionKey, app_id: &str) -> Option<&Entry> {
        if self.entries.contains_key(&key) {
            return None;
        }
        let entry = Entry {
            app_id: app_id.to_owned(),
            state: CastSession::Created,
            selection: None,
        };
        Some(self.entries.entry(key).or_insert(entry))
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

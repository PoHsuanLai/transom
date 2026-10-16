//! The held requests, the effect on the session and the monitors.

use std::collections::BTreeMap;

use transom_core::{InhibitBook, InhibitFlags, RequestHandle};

use super::effect::{EffectError, InhibitEffect};
use crate::peers::{Admission, HOLD_LIMITS, Tether, admit};

/// The most monitors at once. A monitor holds nothing, but the map is the caller's to fill.
pub const MAX_MONITORS: usize = 64;

/// Why a request was not held.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum HoldError {
    /// The caller or the whole hub is at its limit of holds.
    #[error("too many holds are open")]
    TooMany,
    /// The session could not be told; the request was not kept.
    #[error(transparent)]
    Effect(#[from] EffectError),
}

/// What a monitoring session has done.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MonitorState {
    /// Whether the app answered the end-of-session query.
    pub answered_query_end: bool,
}

/// The held requests, the effect on the session and the monitors.
#[derive(Debug)]
pub struct InhibitHub {
    book: InhibitBook,
    effect: Box<dyn InhibitEffect>,
    monitors: BTreeMap<String, MonitorState>,
    /// The peers each held request hangs on.
    owners: BTreeMap<RequestHandle, Tether>,
}

impl InhibitHub {
    /// A hub holding nothing, telling `effect` of every change.
    pub fn new(effect: Box<dyn InhibitEffect>) -> Self {
        Self {
            book: InhibitBook::new(),
            effect,
            monitors: BTreeMap::new(),
            owners: BTreeMap::new(),
        }
    }

    /// Hold a request, hung on `tether`. A request that cannot be held (too many, or the session
    /// could not be told) is not kept: the book never says something is held that the session was
    /// not told about.
    pub async fn hold(
        &mut self,
        handle: RequestHandle,
        flags: InhibitFlags,
        tether: Tether,
    ) -> Result<(), HoldError> {
        let replacing = self.owners.contains_key(&handle);
        if !replacing && admit(self.owners.values(), &tether, HOLD_LIMITS) == Admission::Full {
            return Err(HoldError::TooMany);
        }
        let change = self.book.hold(handle.clone(), flags);
        self.owners.insert(handle.clone(), tether);
        if let Err(error) = self.tell(change).await {
            self.book.release(&handle);
            self.owners.remove(&handle);
            return Err(error.into());
        }
        Ok(())
    }

    /// Release a request; a handle not held does nothing.
    pub async fn release(&mut self, handle: &RequestHandle) -> Result<(), EffectError> {
        self.owners.remove(handle);
        let change = self.book.release(handle);
        self.tell(change).await
    }

    /// Release every request hung on `peer` (a unique name that left the bus, or stopped being
    /// the portal frontend); the handles released. An effect that fails is logged: the requests
    /// are gone from the book either way.
    pub async fn release_peer(&mut self, peer: &str) -> Vec<RequestHandle> {
        let handles: Vec<RequestHandle> = self
            .owners
            .iter()
            .filter(|(_, tether)| tether.holds(peer))
            .map(|(handle, _)| handle.clone())
            .collect();
        for handle in &handles {
            if let Err(error) = self.release(handle).await {
                tracing::warn!(%error, "a hold of a peer that left could not be passed on");
            }
        }
        handles
    }

    async fn tell(&mut self, change: transom_core::BookChange) -> Result<(), EffectError> {
        if change.is_change() {
            self.effect.apply(change.after).await
        } else {
            Ok(())
        }
    }

    /// The union of what is held.
    pub fn active(&self) -> InhibitFlags {
        self.book.active()
    }

    /// How many requests are held.
    pub fn held(&self) -> usize {
        self.book.len()
    }

    /// Start a monitor; `AlreadyThere` when that session already has one, `Full` at
    /// [`MAX_MONITORS`].
    pub fn add_monitor(&mut self, session: &str) -> MonitorAdded {
        if self.monitors.len() >= MAX_MONITORS && !self.monitors.contains_key(session) {
            return MonitorAdded::Full;
        }
        match self.monitors.entry(session.to_owned()) {
            std::collections::btree_map::Entry::Vacant(v) => {
                v.insert(MonitorState::default());
                MonitorAdded::New
            }
            std::collections::btree_map::Entry::Occupied(_) => MonitorAdded::AlreadyThere,
        }
    }

    /// Stop a monitor.
    pub fn remove_monitor(&mut self, session: &str) {
        self.monitors.remove(session);
    }

    /// Note that a monitor answered the end-of-session query; false for an unknown session.
    pub fn note_answer(&mut self, session: &str) -> MonitorKnown {
        match self.monitors.get_mut(session) {
            Some(state) => {
                state.answered_query_end = true;
                MonitorKnown::Known
            }
            None => MonitorKnown::Unknown,
        }
    }

    /// A monitor's state.
    pub fn monitor(&self, session: &str) -> Option<MonitorState> {
        self.monitors.get(session).copied()
    }
}

/// Whether a monitor was new.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MonitorAdded {
    /// It was started.
    New,
    /// The session was already monitored.
    AlreadyThere,
    /// There are too many monitors already.
    Full,
}

/// Whether a session is a monitor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MonitorKnown {
    /// It is.
    Known,
    /// It is not.
    Unknown,
}

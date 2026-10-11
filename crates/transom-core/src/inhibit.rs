//! The book of "keep the screen on" requests (the Inhibit portal).
//!
//! Each request is held under the handle of its portal request object. When the object goes
//! away the request is released; the book says what the union of the held flags became so the
//! caller knows whether to tell the session.

use std::collections::BTreeMap;

/// What an app asked to prevent. The wire values are the portal's bit set.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InhibitFlags(u32);

impl InhibitFlags {
    /// Nothing.
    pub const NONE: Self = Self(0);
    /// Logging out.
    pub const LOGOUT: Self = Self(1);
    /// Switching the user.
    pub const USER_SWITCH: Self = Self(2);
    /// Suspending.
    pub const SUSPEND: Self = Self(4);
    /// Going idle (the screen blanking or locking).
    pub const IDLE: Self = Self(8);

    /// The flags for a portal value; unknown bits are dropped.
    pub fn from_wire(value: u32) -> Self {
        Self(value & 15)
    }

    /// The portal's value.
    pub fn to_wire(self) -> u32 {
        self.0
    }

    /// Both sets together.
    pub fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether every flag of `other` is in `self`.
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

/// The handle of one request: the object path of its portal request.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RequestHandle(pub String);

/// What a change to the book did to the union.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BookChange {
    /// The union before.
    pub before: InhibitFlags,
    /// The union after.
    pub after: InhibitFlags,
}

impl BookChange {
    /// Whether the union is different.
    pub fn is_change(&self) -> bool {
        self.before != self.after
    }
}

/// Every held request.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InhibitBook {
    held: BTreeMap<RequestHandle, InhibitFlags>,
}

impl InhibitBook {
    /// An empty book.
    pub fn new() -> Self {
        Self::default()
    }

    /// The union of everything held.
    pub fn active(&self) -> InhibitFlags {
        self.held
            .values()
            .fold(InhibitFlags::NONE, |acc, f| acc.union(*f))
    }

    /// Hold a request. A handle already held is replaced.
    pub fn hold(&mut self, handle: RequestHandle, flags: InhibitFlags) -> BookChange {
        let before = self.active();
        self.held.insert(handle, flags);
        BookChange {
            before,
            after: self.active(),
        }
    }

    /// Release a request. A handle that is not held changes nothing.
    pub fn release(&mut self, handle: &RequestHandle) -> BookChange {
        let before = self.active();
        self.held.remove(handle);
        BookChange {
            before,
            after: self.active(),
        }
    }

    /// How many requests are held.
    pub fn len(&self) -> usize {
        self.held.len()
    }

    /// Whether nothing is held.
    pub fn is_empty(&self) -> bool {
        self.held.is_empty()
    }
}

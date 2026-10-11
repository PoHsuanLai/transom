//! Restore tokens: how an app that shared a screen once can share it again without asking.
//!
//! A token stands for an app and the sources it was given. It is a bearer secret, so its `Debug`
//! is redacted and nothing here logs it.

use std::collections::BTreeMap;
use std::fmt;

use crate::screencast::CastSource;

/// How long a grant is remembered. The wire values are the portal's: 0, 1, 2.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum PersistMode {
    /// Not at all.
    DoNot,
    /// While the app that asked is running.
    WhileRunning,
    /// Until the person takes it back.
    UntilRevoked,
}

impl PersistMode {
    /// The mode for a portal value; `None` for anything but 0, 1 or 2.
    pub fn from_wire(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::DoNot),
            1 => Some(Self::WhileRunning),
            2 => Some(Self::UntilRevoked),
            _ => None,
        }
    }

    /// The portal's value for this mode.
    pub fn to_wire(self) -> u32 {
        match self {
            Self::DoNot => 0,
            Self::WhileRunning => 1,
            Self::UntilRevoked => 2,
        }
    }
}

/// An opaque restore token.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RestoreToken(String);

impl RestoreToken {
    /// A token made from 16 bytes the caller drew from a random source: 32 lower-case hex digits.
    pub fn mint(entropy: [u8; 16]) -> Self {
        const DIGITS: &[u8; 16] = b"0123456789abcdef";
        let text = entropy
            .iter()
            .flat_map(|byte| {
                [
                    DIGITS[usize::from(byte >> 4)],
                    DIGITS[usize::from(byte & 15)],
                ]
            })
            .map(char::from)
            .collect();
        Self(text)
    }

    /// A token read back from the wire or a file; `None` unless it is 32 lower-case hex digits.
    pub fn parse(text: &str) -> Option<Self> {
        let shaped =
            text.len() == 32 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        shaped.then(|| Self(text.to_owned()))
    }

    /// The text to put on the wire. Handle with care: whoever holds it can ask for the same
    /// sources.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for RestoreToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RestoreToken(<redacted>)")
    }
}

/// What one token stands for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    /// The app the token was given to; another app's token is refused.
    pub app_id: String,
    /// The sources it was given.
    pub sources: Vec<CastSource>,
}

/// Every token this portal remembers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RestoreBook {
    grants: BTreeMap<RestoreToken, Grant>,
}

impl RestoreBook {
    /// An empty book.
    pub fn new() -> Self {
        Self::default()
    }

    /// Remember a grant under a token, replacing any earlier one under the same token.
    pub fn remember(&mut self, token: RestoreToken, grant: Grant) {
        self.grants.insert(token, grant);
    }

    /// Forget a token (the person took the access back, or the app asked for a fresh one).
    pub fn forget(&mut self, token: &RestoreToken) -> Option<Grant> {
        self.grants.remove(token)
    }

    /// What `token` restores for `app_id` given the sources that exist now.
    ///
    /// All or nothing: if the token is unknown, belongs to another app, or any remembered source
    /// is gone, the result is empty and the person is asked again.
    pub fn resolve(
        &self,
        token: &RestoreToken,
        app_id: &str,
        live: &[CastSource],
    ) -> Vec<CastSource> {
        match self.grants.get(token) {
            Some(grant)
                if grant.app_id == app_id
                    && !grant.sources.is_empty()
                    && grant.sources.iter().all(|s| live.contains(s)) =>
            {
                grant.sources.clone()
            }
            _ => Vec::new(),
        }
    }

    /// Every remembered grant, in token order (for a store to write).
    pub fn iter(&self) -> impl Iterator<Item = (&RestoreToken, &Grant)> {
        self.grants.iter()
    }

    /// How many grants are remembered.
    pub fn len(&self) -> usize {
        self.grants.len()
    }

    /// Whether the book is empty.
    pub fn is_empty(&self) -> bool {
        self.grants.is_empty()
    }
}

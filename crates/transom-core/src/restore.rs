//! Restore tokens: how an app that shared a screen once can share it again without asking.
//!
//! A token stands for an app and the sources it was given. It is a bearer secret, so its `Debug`
//! is redacted and nothing here logs it. A grant is remembered for a limited time, by the mode the
//! app asked for, and never for an app with no identity: two apps that both arrive with an empty
//! app id would otherwise share each other's grants, so such an app is asked every time.

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

/// How long a grant for [`PersistMode::WhileRunning`] is remembered: a day. The portal cannot see
/// the app end, so "while it runs" is bounded by the longest a session of one app plausibly lasts.
pub const WHILE_RUNNING_SECS: u64 = 24 * 60 * 60;

/// How long a grant for [`PersistMode::UntilRevoked`] is remembered: thirty days, the interval at
/// which macOS asks again whether an app may keep recording the screen. After that the person is
/// asked once more, and a new grant starts a new thirty days.
pub const UNTIL_REVOKED_SECS: u64 = 30 * 24 * 60 * 60;

/// The most grants the book holds; when more are added the oldest are forgotten.
pub const MAX_GRANTS: usize = 256;

/// What one token stands for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    /// The app the token was given to; another app's token is refused.
    pub app_id: String,
    /// The sources it was given.
    pub sources: Vec<CastSource>,
    /// How long the app asked for it to be remembered.
    pub mode: PersistMode,
    /// When it was given, in seconds since the Unix epoch.
    pub granted_at: u64,
}

impl Grant {
    /// The grant to remember for `app_id`, or `None` when none may be kept: the app asked for no
    /// persistence, has no identity (an empty app id is shared by every app that has none), or
    /// shared nothing.
    pub fn issue(
        app_id: &str,
        sources: Vec<CastSource>,
        mode: PersistMode,
        now: u64,
    ) -> Option<Self> {
        let keeps = matches!(mode, PersistMode::WhileRunning | PersistMode::UntilRevoked);
        (keeps && !app_id.is_empty() && !sources.is_empty()).then(|| Self {
            app_id: app_id.to_owned(),
            sources,
            mode,
            granted_at: now,
        })
    }

    /// Whether the grant still counts at `now`: younger than its mode allows, and not dated in
    /// the future (a clock that moved back, or a file that was edited).
    pub fn valid_at(&self, now: u64) -> bool {
        let limit = match self.mode {
            PersistMode::WhileRunning => WHILE_RUNNING_SECS,
            PersistMode::UntilRevoked => UNTIL_REVOKED_SECS,
            _ => return false,
        };
        now.checked_sub(self.granted_at)
            .is_some_and(|age| age <= limit)
    }
}

/// What the person sees of one app's remembered access: no token, nothing secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Remembered {
    /// The app.
    pub app_id: String,
    /// When its newest grant was given, in seconds since the Unix epoch.
    pub granted_at: u64,
    /// The longest mode among its grants.
    pub mode: PersistMode,
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

    /// What `token` restores for `app_id` at time `now` given the sources that exist now.
    ///
    /// All or nothing: if the token is unknown, expired, belongs to another app, `app_id` is
    /// empty, or any remembered source is gone, the result is empty and the person is asked again.
    pub fn resolve(
        &self,
        token: &RestoreToken,
        app_id: &str,
        live: &[CastSource],
        now: u64,
    ) -> Vec<CastSource> {
        match self.grants.get(token) {
            Some(grant)
                if !app_id.is_empty()
                    && grant.app_id == app_id
                    && grant.valid_at(now)
                    && !grant.sources.is_empty()
                    && grant.sources.iter().all(|s| live.contains(s)) =>
            {
                grant.sources.clone()
            }
            _ => Vec::new(),
        }
    }

    /// Forget the grants that no longer count at `now`, and the oldest ones past [`MAX_GRANTS`].
    /// How many were forgotten.
    pub fn prune(&mut self, now: u64) -> usize {
        let before = self.grants.len();
        self.grants.retain(|_, grant| grant.valid_at(now));
        if self.grants.len() > MAX_GRANTS {
            let mut by_age: Vec<(u64, RestoreToken)> = self
                .grants
                .iter()
                .map(|(token, grant)| (grant.granted_at, token.clone()))
                .collect();
            by_age.sort();
            let surplus = self.grants.len() - MAX_GRANTS;
            for (_, token) in by_age.into_iter().take(surplus) {
                self.grants.remove(&token);
            }
        }
        before - self.grants.len()
    }

    /// The apps with a grant that still counts at `now`, one entry each, by app id: what a list
    /// the person can edit shows. Tokens are not part of it.
    pub fn remembered(&self, now: u64) -> Vec<Remembered> {
        let mut by_app: BTreeMap<&str, Remembered> = BTreeMap::new();
        for grant in self.grants.values().filter(|g| g.valid_at(now)) {
            let entry = by_app
                .entry(grant.app_id.as_str())
                .or_insert_with(|| Remembered {
                    app_id: grant.app_id.clone(),
                    granted_at: grant.granted_at,
                    mode: grant.mode,
                });
            entry.granted_at = entry.granted_at.max(grant.granted_at);
            entry.mode = entry.mode.max(grant.mode);
        }
        by_app.into_values().collect()
    }

    /// Forget every grant of `app_id` (the person took the access back). How many were forgotten.
    pub fn forget_app(&mut self, app_id: &str) -> usize {
        let before = self.grants.len();
        self.grants.retain(|_, grant| grant.app_id != app_id);
        before - self.grants.len()
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

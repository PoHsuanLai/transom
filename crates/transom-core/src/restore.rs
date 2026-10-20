//! Restore tokens: how an app that shared a screen once can share it again without asking.
//!
//! A token stands for an app and the sources it was given. It is a bearer secret, so its `Debug`
//! is redacted and nothing here logs it. A grant is remembered for a limited time, by the mode the
//! app asked for, and never for an app with no identity: two apps that both arrive with an empty
//! app id would otherwise share each other's grants, so such an app is asked every time.

use std::collections::BTreeMap;
use std::fmt;

use crate::devices::DeviceKinds;
use crate::screencast::CastSource;

/// How long a stored grant is remembered. The wire values are the portal's: 1 and 2.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Keep {
    /// While the app that asked is running.
    WhileRunning,
    /// Until the person takes it back.
    UntilRevoked,
}

impl Keep {
    /// The keep for a portal value; `None` for anything but 1 or 2.
    pub fn from_wire(value: u32) -> Option<Self> {
        match value {
            1 => Some(Self::WhileRunning),
            2 => Some(Self::UntilRevoked),
            _ => None,
        }
    }

    /// The portal's value for this keep.
    pub fn to_wire(self) -> u32 {
        match self {
            Self::WhileRunning => 1,
            Self::UntilRevoked => 2,
        }
    }
}

/// What an app asks to have remembered. The wire values are the portal's: 0, 1, 2.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum PersistMode {
    /// Not at all.
    DoNot,
    /// Remember it, for this long.
    Keep(Keep),
}

impl PersistMode {
    /// The mode for a portal value; `None` for anything but 0, 1 or 2.
    pub fn from_wire(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::DoNot),
            other => Keep::from_wire(other).map(Self::Keep),
        }
    }

    /// The portal's value for this mode.
    pub fn to_wire(self) -> u32 {
        match self {
            Self::DoNot => 0,
            Self::Keep(keep) => keep.to_wire(),
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

/// How long a grant for [`Keep::WhileRunning`] is remembered: a day. The portal cannot see
/// the app end, so "while it runs" is bounded by the longest a session of one app plausibly lasts.
pub const WHILE_RUNNING_SECS: u64 = 24 * 60 * 60;

/// How long a grant for [`Keep::UntilRevoked`] is remembered: thirty days, the interval at
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
    pub keep: Keep,
    /// When it was given, in seconds since the Unix epoch.
    pub granted_at: u64,
}

/// Whether a grant of `keep` given at `granted_at` still counts at `now`: younger than its mode
/// allows, and not dated in the future (a clock that moved back, or a file that was edited).
fn counts(keep: Keep, granted_at: u64, now: u64) -> bool {
    let limit = match keep {
        Keep::WhileRunning => WHILE_RUNNING_SECS,
        Keep::UntilRevoked => UNTIL_REVOKED_SECS,
    };
    now.checked_sub(granted_at).is_some_and(|age| age <= limit)
}

impl Grant {
    /// The grant to remember for `app_id`, or `None` when none may be kept: the app has no identity (an empty app id is shared by every app that has none), or
    /// shared nothing.
    pub fn issue(app_id: &str, sources: Vec<CastSource>, keep: Keep, now: u64) -> Option<Self> {
        (!app_id.is_empty() && !sources.is_empty()).then(|| Self {
            app_id: app_id.to_owned(),
            sources,
            keep,
            granted_at: now,
        })
    }

    /// Whether the grant still counts at `now`.
    pub fn valid_at(&self, now: u64) -> bool {
        counts(self.keep, self.granted_at, now)
    }
}

/// What a token for remote control stands for: the input kinds the person allowed an app to take
/// over. It is remembered like a [`Grant`], by the same rules, in the same book.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputGrant {
    /// The app the token was given to; another app's token is refused.
    pub app_id: String,
    /// The kinds it was given.
    pub devices: DeviceKinds,
    /// How long the app asked for it to be remembered.
    pub keep: Keep,
    /// When it was given, in seconds since the Unix epoch.
    pub granted_at: u64,
}

impl InputGrant {
    /// The grant to remember for `app_id`, or `None` when none may be kept: the app has no
    /// identity, or was given no device.
    pub fn issue(app_id: &str, devices: DeviceKinds, keep: Keep, now: u64) -> Option<Self> {
        (!app_id.is_empty() && !devices.is_empty()).then(|| Self {
            app_id: app_id.to_owned(),
            devices,
            keep,
            granted_at: now,
        })
    }

    /// Whether the grant still counts at `now`.
    pub fn valid_at(&self, now: u64) -> bool {
        counts(self.keep, self.granted_at, now)
    }
}

/// What the person sees of one app's remembered access: no token, nothing secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Remembered {
    /// The app.
    pub app_id: String,
    /// When its newest grant was given, in seconds since the Unix epoch.
    pub granted_at: u64,
    /// The longest keep among its grants.
    pub keep: Keep,
}

/// Every token this portal remembers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RestoreBook {
    grants: BTreeMap<RestoreToken, Grant>,
    inputs: BTreeMap<RestoreToken, InputGrant>,
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

    /// Remember a remote-control grant under a token, replacing any earlier one under the same
    /// token.
    pub fn remember_input(&mut self, token: RestoreToken, grant: InputGrant) {
        self.inputs.insert(token, grant);
    }

    /// Forget a token of either kind (the person took the access back, or the app asked for a
    /// fresh one).
    pub fn forget(&mut self, token: &RestoreToken) -> Option<Grant> {
        self.inputs.remove(token);
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

    /// What `token` restores for `app_id` at time `now` when it asks for the kinds `wanted`: all
    /// the kinds the grant was given, or none.
    ///
    /// All or nothing, as for [`RestoreBook::resolve`]; and a token that was given fewer kinds
    /// than are wanted restores nothing, so an app cannot grow what it was allowed. An app that
    /// asks for fewer gets what it asks for, and the grant it is remembered by keeps all its kinds.
    pub fn resolve_input(
        &self,
        token: &RestoreToken,
        app_id: &str,
        wanted: DeviceKinds,
        now: u64,
    ) -> DeviceKinds {
        match self.inputs.get(token) {
            Some(grant)
                if !app_id.is_empty()
                    && grant.app_id == app_id
                    && grant.valid_at(now)
                    && grant.devices.covers(wanted) =>
            {
                grant.devices
            }
            _ => DeviceKinds::NONE,
        }
    }

    /// Forget the grants that no longer count at `now`, and the oldest ones past [`MAX_GRANTS`].
    /// How many were forgotten.
    pub fn prune(&mut self, now: u64) -> usize {
        let before = self.len();
        self.grants.retain(|_, grant| grant.valid_at(now));
        self.inputs.retain(|_, grant| grant.valid_at(now));
        let surplus = self.len().saturating_sub(MAX_GRANTS);
        if surplus > 0 {
            let mut by_age: Vec<(u64, RestoreToken)> = self
                .grants
                .iter()
                .map(|(token, grant)| (grant.granted_at, token.clone()))
                .chain(
                    self.inputs
                        .iter()
                        .map(|(token, grant)| (grant.granted_at, token.clone())),
                )
                .collect();
            by_age.sort();
            for (_, token) in by_age.into_iter().take(surplus) {
                self.grants.remove(&token);
                self.inputs.remove(&token);
            }
        }
        before - self.len()
    }

    /// The apps with a grant, of either kind, that still counts at `now`, one entry each, by app
    /// id: what a list the person can edit shows. Tokens are not part of it.
    pub fn remembered(&self, now: u64) -> Vec<Remembered> {
        let screens = self
            .grants
            .values()
            .filter(|g| g.valid_at(now))
            .map(|g| (g.app_id.as_str(), g.granted_at, g.keep));
        let inputs = self
            .inputs
            .values()
            .filter(|g| g.valid_at(now))
            .map(|g| (g.app_id.as_str(), g.granted_at, g.keep));
        let mut by_app: BTreeMap<&str, Remembered> = BTreeMap::new();
        for (app_id, granted_at, keep) in screens.chain(inputs) {
            let entry = by_app.entry(app_id).or_insert_with(|| Remembered {
                app_id: app_id.to_owned(),
                granted_at,
                keep,
            });
            entry.granted_at = entry.granted_at.max(granted_at);
            entry.keep = entry.keep.max(keep);
        }
        by_app.into_values().collect()
    }

    /// Forget every grant of `app_id`, of either kind (the person took the access back). How many
    /// were forgotten.
    pub fn forget_app(&mut self, app_id: &str) -> usize {
        let before = self.len();
        self.grants.retain(|_, grant| grant.app_id != app_id);
        self.inputs.retain(|_, grant| grant.app_id != app_id);
        before - self.len()
    }

    /// Every remembered screen grant, in token order (for a store to write).
    pub fn iter(&self) -> impl Iterator<Item = (&RestoreToken, &Grant)> {
        self.grants.iter()
    }

    /// Every remembered remote-control grant, in token order (for a store to write).
    pub fn inputs(&self) -> impl Iterator<Item = (&RestoreToken, &InputGrant)> {
        self.inputs.iter()
    }

    /// How many grants of either kind are remembered.
    pub fn len(&self) -> usize {
        self.grants.len() + self.inputs.len()
    }

    /// Whether the book is empty.
    pub fn is_empty(&self) -> bool {
        self.grants.is_empty() && self.inputs.is_empty()
    }
}

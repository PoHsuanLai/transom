//! A standing grant: the person, in Settings, lets one app share the screen and take over the
//! keyboard and pointer without being asked each time (a remote-access app that is used while
//! nobody sits at the machine).
//!
//! It differs from a token grant in three ways. The person makes it, not the app, so it does not
//! depend on the app asking to be remembered (some never do). It names a rule, not sources, so it
//! still works when the screens change. And it does not run out: it stays until it is taken back.

use crate::devices::DeviceKinds;
use crate::screencast::CastSource;

/// Which screens a standing grant lets an app share.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Screens {
    /// Every display there is when the app asks.
    Every,
    /// This display by its output name; when it is not there, the first one that is.
    Named(String),
}

impl Screens {
    /// The sources this rule stands for among the `live` ones: outputs only, never a window.
    /// Empty when there is no display to share.
    pub fn pick(&self, live: &[CastSource]) -> Vec<CastSource> {
        let outputs: Vec<&CastSource> = live
            .iter()
            .filter(|s| matches!(s, CastSource::Output(_)))
            .collect();
        match self {
            Self::Every => outputs.into_iter().cloned().collect(),
            Self::Named(name) => outputs
                .iter()
                .find(|s| matches!(s, CastSource::Output(n) if n == name))
                .or(outputs.first())
                .map(|s| vec![(*s).clone()])
                .unwrap_or_default(),
        }
    }
}

/// What the person let one app do without asking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StandingGrant {
    /// The app, by the id it is proven to have.
    pub app_id: String,
    /// The displays it may share; `None` when it may not share the screen without asking.
    pub screens: Option<Screens>,
    /// The input it may take over; none when it may not.
    pub input: DeviceKinds,
    /// When the person gave it, in seconds since the Unix epoch.
    pub granted_at: u64,
}

impl StandingGrant {
    /// The grant for `app_id`, or `None` when none may be kept: the app has no identity (an empty
    /// id is shared by every app that has none), or nothing is allowed.
    pub fn issue(
        app_id: &str,
        screens: Option<Screens>,
        input: DeviceKinds,
        granted_at: u64,
    ) -> Option<Self> {
        let something = screens.is_some() || !input.is_empty();
        (!app_id.is_empty() && something).then(|| Self {
            app_id: app_id.to_owned(),
            screens,
            input,
            granted_at,
        })
    }
}

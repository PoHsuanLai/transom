//! What the person may pick when asked what to share, and the checks on the answer.
//!
//! The chooser is a surface of the shell; this module only says what to offer and whether the
//! answer is acceptable.

use crate::screencast::{CastSource, CursorMode};

/// Which kinds of source an app asked for. Built from the portal's `types` bit set (1 display,
/// 2 window); other bits are dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SourceKinds {
    /// Displays only.
    Outputs,
    /// Windows only.
    Windows,
    /// Both.
    Both,
}

impl SourceKinds {
    /// The kinds for a portal value. An empty set (the app sent 0 or only unknown bits) means
    /// displays, the portal's own default.
    pub fn from_wire(value: u32) -> Self {
        match value & 3 {
            2 => Self::Windows,
            3 => Self::Both,
            _ => Self::Outputs,
        }
    }

    /// The portal's value.
    pub fn to_wire(self) -> u32 {
        match self {
            Self::Outputs => 1,
            Self::Windows => 2,
            Self::Both => 3,
        }
    }

    /// Whether `source` is of an offered kind.
    pub fn offers(self, source: &CastSource) -> bool {
        matches!(
            (self, source),
            (Self::Both, _)
                | (Self::Outputs, CastSource::Output(_))
                | (Self::Windows, CastSource::Window(_))
        )
    }
}

/// Whether the app may be given more than one source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Multiple {
    /// Exactly one.
    One,
    /// One or more.
    Many,
}

/// What the chooser is asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChoiceRequest {
    /// The kinds to offer.
    pub kinds: SourceKinds,
    /// One or several.
    pub multiple: Multiple,
    /// The cursor modes the app allows; the person's pick must be one of them.
    pub cursor_modes: Vec<CursorMode>,
    /// The app's name as the person knows it (never a bundle or bus name), for the chooser's
    /// title.
    pub app_label: String,
}

/// What the person picked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    /// The sources to share.
    pub sources: Vec<CastSource>,
    /// How the pointer appears.
    pub cursor: CursorMode,
}

/// Why an answer was not acceptable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ChoiceRefusal {
    /// Nothing was picked.
    #[error("nothing was picked")]
    Empty,
    /// More than one source for an app that asked for one.
    #[error("more than one source was picked")]
    TooMany,
    /// A source of a kind the app did not ask for.
    #[error("a source of an unrequested kind was picked")]
    WrongKind,
    /// A cursor mode the app does not allow.
    #[error("the cursor mode is not allowed")]
    CursorNotAllowed,
    /// The same source twice.
    #[error("a source was picked twice")]
    Duplicate,
}

impl ChoiceRequest {
    /// Check an answer against the request.
    pub fn accept(
        &self,
        sources: Vec<CastSource>,
        cursor: CursorMode,
    ) -> Result<Choice, ChoiceRefusal> {
        if sources.is_empty() {
            return Err(ChoiceRefusal::Empty);
        }
        if self.multiple == Multiple::One && sources.len() > 1 {
            return Err(ChoiceRefusal::TooMany);
        }
        if !sources.iter().all(|s| self.kinds.offers(s)) {
            return Err(ChoiceRefusal::WrongKind);
        }
        let mut seen = sources.clone();
        seen.sort();
        seen.dedup();
        if seen.len() != sources.len() {
            return Err(ChoiceRefusal::Duplicate);
        }
        if !self.cursor_modes.contains(&cursor) {
            return Err(ChoiceRefusal::CursorNotAllowed);
        }
        Ok(Choice { sources, cursor })
    }
}

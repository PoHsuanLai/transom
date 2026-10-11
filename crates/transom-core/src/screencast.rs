//! The state machine of one screen-sharing session.
//!
//! A session is created, told (or not told) what to share, started, and closed. [`step`] takes
//! the session and one input and returns the next session and what the caller must do. Nothing
//! here opens a stream or shows a chooser; the outputs say so and the caller does it.

/// A PipeWire node id: the stream an app connects to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PwNode(pub u32);

/// One thing that can be shared.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum CastSource {
    /// A whole display, by its output name.
    Output(String),
    /// One window, by the toplevel identifier the compositor publishes.
    Window(String),
}

impl CastSource {
    /// The chooser's wire form: `("output", name)` or `("window", identifier)`.
    ///
    /// ```
    /// use transom_core::screencast::CastSource;
    /// assert_eq!(CastSource::Output("DP-1".into()).wire(), ("output", "DP-1"));
    /// ```
    pub fn wire(&self) -> (&'static str, &str) {
        match self {
            CastSource::Output(name) => ("output", name),
            CastSource::Window(id) => ("window", id),
        }
    }
}

/// How the pointer appears in the shared picture. The wire values are the portal's: 1, 2, 4.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum CursorMode {
    /// Not in the picture.
    Hidden,
    /// Drawn into the picture.
    Embedded,
    /// Sent beside the picture as stream metadata.
    Metadata,
}

impl CursorMode {
    /// The portal's value for this mode.
    pub fn to_wire(self) -> u32 {
        match self {
            Self::Hidden => 1,
            Self::Embedded => 2,
            Self::Metadata => 4,
        }
    }

    /// The mode for a portal value; `None` for anything but exactly 1, 2 or 4.
    pub fn from_wire(value: u32) -> Option<Self> {
        match value {
            1 => Some(Self::Hidden),
            2 => Some(Self::Embedded),
            4 => Some(Self::Metadata),
            _ => None,
        }
    }
}

/// Where a session stands.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CastSession {
    /// Made, nothing chosen.
    Created,
    /// The sources are known and the stream is being opened.
    SourcesSelected(Vec<CastSource>),
    /// The stream is live on this node.
    Started {
        /// The node an app reads.
        node: PwNode,
    },
    /// Over; every input is ignored.
    Closed,
}

/// What reaches a session.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CastIn {
    /// The app called Start. `restored` is what its restore token resolved to now, empty if it
    /// had none or the sources are gone.
    Start {
        /// Sources a valid restore token stands for.
        restored: Vec<CastSource>,
    },
    /// The person answered the chooser; empty means they cancelled.
    Picked(Vec<CastSource>),
    /// The stream is open on this node.
    NodeReady(PwNode),
    /// The stream could not be opened.
    StreamFailed,
    /// The source went away while sharing (the window closed, the display unplugged).
    StreamEnded,
    /// The app, or the portal, closed the session.
    Close,
}

/// What the caller must do.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CastOut {
    /// Ask the person what to share.
    AskChooser,
    /// Open a stream for these sources.
    OpenStream(Vec<CastSource>),
    /// Tear the stream down.
    CloseStream,
    /// Answer the app's Start call.
    Reply(CastReply),
}

/// The answer to Start.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CastReply {
    /// Sharing began.
    Granted {
        /// The node to read.
        node: PwNode,
        /// What is shared.
        sources: Vec<CastSource>,
    },
    /// The person said no.
    Cancelled,
    /// Something broke.
    Failed,
}

/// One transition.
///
/// Whether to hand back a restore token is the caller's business: it records the app's persist
/// mode when the app selects sources and mints a token when it sees [`CastReply::Granted`].
pub fn step(session: CastSession, input: CastIn) -> (CastSession, Vec<CastOut>) {
    use CastIn as I;
    use CastOut as O;
    use CastSession as S;
    match (session, input) {
        (S::Closed, _) => (S::Closed, Vec::new()),

        (S::Created, I::Start { restored }) if !restored.is_empty() => (
            S::SourcesSelected(restored.clone()),
            vec![O::OpenStream(restored)],
        ),
        (S::Created, I::Start { .. }) => (S::Created, vec![O::AskChooser]),
        (S::Created, I::Picked(sources)) if !sources.is_empty() => (
            S::SourcesSelected(sources.clone()),
            vec![O::OpenStream(sources)],
        ),
        (S::Created, I::Picked(_)) => (S::Closed, vec![O::Reply(CastReply::Cancelled)]),
        (S::Created, I::Close) => (S::Closed, Vec::new()),

        (S::SourcesSelected(sources), I::NodeReady(node)) => (
            S::Started { node },
            vec![O::Reply(CastReply::Granted { node, sources })],
        ),
        (S::SourcesSelected(_), I::StreamFailed | I::StreamEnded) => {
            (S::Closed, vec![O::CloseStream, O::Reply(CastReply::Failed)])
        }
        (S::SourcesSelected(_), I::Close) => (S::Closed, vec![O::CloseStream]),

        (S::Started { .. }, I::StreamFailed | I::StreamEnded | I::Close) => {
            (S::Closed, vec![O::CloseStream])
        }

        // Anything else is out of order (a second Start, a pick after the stream opened): the
        // session does not move and nothing happens.
        (session, _) => (session, Vec::new()),
    }
}

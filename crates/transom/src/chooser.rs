//! Asking the person what to share.
//!
//! The picker is a surface of the desktop shell, not of this process: [`ShellChooser`] calls
//! `org.quire.Shell1.PickCaptureSource` on the shell's bus name and returns its answer.
//! [`FakeChooser`] answers from a script, for tests.

use std::sync::Mutex;

use transom_core::{CastSource, Choice, ChoiceRequest, CursorMode};

use crate::BoxFuture;

/// Why the person could not be asked.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ChooserError {
    /// The shell is not on the bus, so there is nobody to ask. Sharing is refused.
    #[error("no picker is available")]
    Unavailable,
    /// The shell answered with something that does not make sense.
    #[error("the picker's answer was not understood")]
    BadAnswer,
    /// The call failed.
    #[error("the picker call failed: {0}")]
    Call(String),
}

/// The seam: show the picker and return what was picked, or `None` if the person cancelled.
pub trait Chooser: Send + Sync + std::fmt::Debug {
    /// Ask. The answer is checked against `request` by the caller.
    fn pick<'a>(
        &'a self,
        request: &'a ChoiceRequest,
    ) -> BoxFuture<'a, Result<Option<Choice>, ChooserError>>;
}

/// The shell's side of the handshake, as the client sees it.
///
/// `PickCaptureSource(app_label: s, kinds: u, multiple: u, cursor_modes: au) -> (sources: a(ss),
/// cursor: u)`. `kinds` is the portal's bit set (1 display, 2 window); `multiple` is 0 for one
/// source and 1 for several; each source is `("output", name)` or `("window", identifier)`; an
/// empty list means the person cancelled. The shell lists what exists, so the call carries no
/// candidates.
#[zbus::proxy(
    interface = "org.quire.Shell1",
    default_service = "org.quire.Shell",
    default_path = "/org/quire/Shell1",
    gen_blocking = false
)]
pub trait Shell1 {
    /// Show the picker and wait for the answer.
    fn pick_capture_source(
        &self,
        app_label: &str,
        kinds: u32,
        multiple: u32,
        cursor_modes: Vec<u32>,
    ) -> zbus::Result<(Vec<(String, String)>, u32)>;
}

/// The real chooser: the desktop shell's picker over the session bus.
#[derive(Debug, Default)]
pub struct ShellChooser;

impl ShellChooser {
    /// A chooser that connects to the session bus on first use.
    pub fn new() -> Self {
        Self
    }
}

impl Chooser for ShellChooser {
    fn pick<'a>(
        &'a self,
        request: &'a ChoiceRequest,
    ) -> BoxFuture<'a, Result<Option<Choice>, ChooserError>> {
        Box::pin(async move {
            let _ = request;
            todo!(
                "P2-T2-CH: call Shell1Proxy::pick_capture_source and map the answer with source_from_wire"
            )
        })
    }
}

/// Map one wire source to a [`CastSource`]; `None` for a kind we do not know.
pub fn source_from_wire(kind: &str, name: &str) -> Option<CastSource> {
    match kind {
        "output" => Some(CastSource::Output(name.to_owned())),
        "window" => Some(CastSource::Window(name.to_owned())),
        _ => None,
    }
}

/// The wire form of a source.
pub fn source_to_wire(source: &CastSource) -> (&'static str, &str) {
    source.wire()
}

/// One scripted answer: the sources and cursor picked, or `None` for a cancel.
type Answer = Option<(Vec<CastSource>, CursorMode)>;

/// A chooser that answers from a script: each call takes the next answer; when the script is
/// empty the person cancels.
#[derive(Debug, Default)]
pub struct FakeChooser {
    answers: Mutex<Vec<Answer>>,
}

impl FakeChooser {
    /// A chooser that will give these answers in order (`None` is a cancel).
    pub fn new(answers: Vec<Option<(Vec<CastSource>, CursorMode)>>) -> Self {
        let mut answers = answers;
        answers.reverse();
        Self {
            answers: Mutex::new(answers),
        }
    }
}

impl Chooser for FakeChooser {
    fn pick<'a>(
        &'a self,
        request: &'a ChoiceRequest,
    ) -> BoxFuture<'a, Result<Option<Choice>, ChooserError>> {
        let next = self.answers.lock().ok().and_then(|mut a| a.pop()).flatten();
        Box::pin(async move {
            Ok(next.and_then(|(sources, cursor)| request.accept(sources, cursor).ok()))
        })
    }
}

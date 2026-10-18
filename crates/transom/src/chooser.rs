//! Asking the person what to share.
//!
//! The picker is a surface of the desktop shell, not of this process: [`ShellChooser`] calls
//! `org.quire.Shell1.PickCaptureSource` on the shell's bus name and returns its answer.
//! [`FakeChooser`] answers from a script, for tests.

use std::sync::Mutex;

use transom_core::{CastSource, Choice, ChoiceRequest, CursorMode, Multiple};

use crate::BoxFuture;
use crate::link::BusLink;
use crate::shell::{Door, answered};

/// Why the person could not be asked.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ChooserError {
    /// The shell is not on the bus, so there is nobody to ask. Sharing is refused.
    #[error("no picker is available")]
    Unavailable,
    /// Whoever owns the shell's name is not the shell, or could not be confirmed to be. Nobody
    /// was asked, so nobody said yes.
    #[error("the shell could not be confirmed")]
    NotTheShell,
    /// The shell's name changed hands while the person was being asked. The question ends unanswered.
    #[error("the shell changed while it was being asked")]
    ShellChanged,
    /// The shell answered with something that does not make sense.
    #[error("the picker's answer was not understood")]
    BadAnswer,
    /// The call failed.
    #[error("the picker call failed: {0}")]
    Call(String),
}

impl ChooserError {
    /// Whether the person was never really asked: no shell to ask, not the shell, or the shell
    /// replaced mid-question. Each of these is a no, never a yes.
    pub fn is_refusal(&self) -> bool {
        matches!(
            self,
            Self::Unavailable | Self::NotTheShell | Self::ShellChanged
        )
    }
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
///
/// `PickColor(app_label: s) -> (color: a(ddd))` serves the Screenshot portal's colour picker.
///
/// `AskPermission(app_label: s, title: s, subtitle: s, body: s, grant_label: s, deny_label: s)
/// -> (answer: u)` serves the Access portal's yes-or-no question: the shell shows a sheet with
/// the app's icon and name, the three texts, and two buttons labelled as given (the grant button
/// is the default). `answer` is 0 when the person pressed the grant button, 1 when they pressed
/// the deny button, 2 when they closed the sheet (Escape, or the sheet was replaced) without
/// answering. Only the portal backend's name may ask.
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

    /// Show the colour picker and wait for the answer: no tuple is a cancel, one tuple is the
    /// picked colour as red, green and blue, each 0 to 1.
    fn pick_color(&self, app_label: &str) -> zbus::Result<Vec<(f64, f64, f64)>>;

    /// Show a yes-or-no sheet and wait for the answer: 0 granted, 1 denied, 2 dismissed.
    fn ask_permission(
        &self,
        app_label: &str,
        title: &str,
        subtitle: &str,
        body: &str,
        grant_label: &str,
        deny_label: &str,
    ) -> zbus::Result<u32>;
}

/// The real chooser: the desktop shell's picker over the session bus, asked from the connection
/// that owns the portal's name (the shell answers no other).
#[derive(Debug)]
pub struct ShellChooser {
    link: BusLink,
}

impl ShellChooser {
    /// A chooser that asks through `link`.
    pub fn new(link: BusLink) -> Self {
        Self { link }
    }
}

impl Chooser for ShellChooser {
    fn pick<'a>(
        &'a self,
        request: &'a ChoiceRequest,
    ) -> BoxFuture<'a, Result<Option<Choice>, ChooserError>> {
        Box::pin(async move {
            let Door { shell, mut watch } = Door::open(&self.link).await?;
            let cursor_modes = request.cursor_modes.iter().map(|m| m.to_wire()).collect();
            let (sources, cursor) = answered(
                &mut watch,
                shell.pick_capture_source(
                    &request.app_label,
                    request.kinds.to_wire(),
                    multiple_to_wire(request.multiple),
                    cursor_modes,
                ),
            )
            .await?;
            answer_from_wire(&sources, cursor)
        })
    }
}

/// The wire value of [`Multiple`]: 0 for one source, 1 for several.
fn multiple_to_wire(multiple: Multiple) -> u32 {
    match multiple {
        Multiple::One => 0,
        _ => 1,
    }
}

pub(crate) fn call_failed(error: zbus::Error) -> ChooserError {
    ChooserError::Call(error.to_string())
}

/// Whether the bus said nobody can answer: the shell is not on the bus (no owner, nothing to
/// start), or it is there but does not serve the picker yet (it has not exported the object, or
/// does not know the method).
pub fn nobody_answers(error: &zbus::Error) -> bool {
    const GONE: [&str; 5] = [
        "org.freedesktop.DBus.Error.ServiceUnknown",
        "org.freedesktop.DBus.Error.NameHasNoOwner",
        "org.freedesktop.DBus.Error.UnknownObject",
        "org.freedesktop.DBus.Error.UnknownInterface",
        "org.freedesktop.DBus.Error.UnknownMethod",
    ];
    match error {
        zbus::Error::MethodError(name, _, _) => GONE.contains(&name.as_str()),
        zbus::Error::FDO(inner) => matches!(
            **inner,
            zbus::fdo::Error::ServiceUnknown(_)
                | zbus::fdo::Error::NameHasNoOwner(_)
                | zbus::fdo::Error::UnknownObject(_)
                | zbus::fdo::Error::UnknownInterface(_)
                | zbus::fdo::Error::UnknownMethod(_)
        ),
        _ => false,
    }
}

/// What a missing picker means for sharing: nobody was asked, so nobody said yes. An answer that
/// could not be had because there is nobody to ask (see [`ChooserError::is_refusal`]) becomes a cancel, which shares nothing and
/// is the portal's plain "the person said no". Any other failure is still a failure.
pub fn unavailable_is_cancel(
    answer: Result<Option<Choice>, ChooserError>,
) -> Result<Option<Choice>, ChooserError> {
    match answer {
        Err(error) if error.is_refusal() => {
            tracing::warn!(%error, "no picker could be asked, so nothing is shared");
            Ok(None)
        }
        other => other,
    }
}

/// Turn the shell's answer into a [`Choice`]. An empty list is a cancel (`Ok(None)`); a source of
/// an unknown kind or a cursor value that is not one of the three modes is a bad answer.
pub fn answer_from_wire(
    sources: &[(String, String)],
    cursor: u32,
) -> Result<Option<Choice>, ChooserError> {
    if sources.is_empty() {
        return Ok(None);
    }
    let sources = sources
        .iter()
        .map(|(kind, name)| source_from_wire(kind, name))
        .collect::<Option<Vec<_>>>()
        .ok_or(ChooserError::BadAnswer)?;
    let cursor = CursorMode::from_wire(cursor).ok_or(ChooserError::BadAnswer)?;
    Ok(Some(Choice { sources, cursor }))
}

/// Map one wire source to a [`CastSource`]; `None` for a kind we do not know.
pub fn source_from_wire(kind: &str, name: &str) -> Option<CastSource> {
    match kind {
        "output" => Some(CastSource::Output(name.to_owned())),
        "window" => Some(CastSource::Window(name.to_owned())),
        _ => None,
    }
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

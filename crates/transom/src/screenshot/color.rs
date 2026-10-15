//! `PickColor`: the person picks a colour from the screen, in a picker the shell shows.

use std::sync::Mutex;

use transom_core::Rgb;

use crate::BoxFuture;
use crate::chooser::{ChooserError, Shell1Proxy, call_failed, nobody_answers};

/// The seam: show the colour picker and return the colour, or `None` if the person cancelled.
pub trait ColorPicker: Send + Sync + std::fmt::Debug {
    /// Ask. `app_label` is the app's name as the person knows it.
    fn pick<'a>(&'a self, app_label: &'a str) -> BoxFuture<'a, Result<Option<Rgb>, ChooserError>>;
}

/// The real picker: `org.quire.Shell1.PickColor(app_label: s) -> (color: a(ddd))`. The shell
/// answers with no tuple for a cancel and one tuple of red, green and blue (each 0 to 1) for a
/// pick.
#[derive(Debug, Default)]
pub struct ShellColorPicker;

impl ShellColorPicker {
    /// A picker that connects to the session bus on first use.
    pub fn new() -> Self {
        Self
    }
}

impl ColorPicker for ShellColorPicker {
    fn pick<'a>(&'a self, app_label: &'a str) -> BoxFuture<'a, Result<Option<Rgb>, ChooserError>> {
        Box::pin(async move {
            let connection = zbus::Connection::session().await.map_err(call_failed)?;
            let proxy = Shell1Proxy::new(&connection).await.map_err(call_failed)?;
            match proxy.pick_color(app_label).await {
                Ok(picked) => color_from_wire(&picked),
                Err(error) if nobody_answers(&error) => Err(ChooserError::Unavailable),
                Err(error) => Err(call_failed(error)),
            }
        })
    }
}

/// Turn the shell's answer into a colour. No tuple is a cancel; one tuple whose channels are
/// each in 0 to 1 is the colour; anything else is a bad answer.
pub fn color_from_wire(picked: &[(f64, f64, f64)]) -> Result<Option<Rgb>, ChooserError> {
    match picked {
        [] => Ok(None),
        [(r, g, b)] if [r, g, b].iter().all(|c| (0.0..=1.0).contains(*c)) => Ok(Some(Rgb {
            r: *r,
            g: *g,
            b: *b,
        })),
        _ => Err(ChooserError::BadAnswer),
    }
}

/// A picker that always cancels: what a service nobody set up does.
#[derive(Debug, Default)]
pub struct NoColorPicker;

impl ColorPicker for NoColorPicker {
    fn pick<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<Option<Rgb>, ChooserError>> {
        Box::pin(async { Ok(None) })
    }
}

/// A picker that answers from a script, for tests: each call takes the next answer; when the
/// script is empty the person cancels.
#[derive(Debug, Default)]
pub struct FakeColorPicker {
    answers: Mutex<Vec<Option<Rgb>>>,
    asked: Mutex<Vec<String>>,
}

impl FakeColorPicker {
    /// A picker that will give these answers in order (`None` is a cancel).
    pub fn new(answers: Vec<Option<Rgb>>) -> Self {
        let mut answers = answers;
        answers.reverse();
        Self {
            answers: Mutex::new(answers),
            asked: Mutex::new(Vec::new()),
        }
    }

    /// The app labels asked about so far.
    pub fn asked(&self) -> Vec<String> {
        self.asked.lock().map(|a| a.clone()).unwrap_or_default()
    }
}

impl ColorPicker for FakeColorPicker {
    fn pick<'a>(&'a self, app_label: &'a str) -> BoxFuture<'a, Result<Option<Rgb>, ChooserError>> {
        if let Ok(mut asked) = self.asked.lock() {
            asked.push(app_label.to_owned());
        }
        let next = self.answers.lock().ok().and_then(|mut a| a.pop()).flatten();
        Box::pin(async move { Ok(next) })
    }
}

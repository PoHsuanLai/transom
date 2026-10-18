//! Asking the person whether an app may control the computer.
//!
//! The question is its own: it says which input the app would take over, in plain words. It is
//! never the screen-sharing picker, because picking a screen to share is not saying yes to this.

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use super::devices::DeviceKinds;
use crate::BoxFuture;
use crate::access::{Answer, Asker, Question};
use crate::chooser::ShellError;

/// What the person said to "let this app control the computer".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConsentAnswer {
    /// Allowed.
    Allowed,
    /// Not allowed (or the question was dismissed).
    Declined,
}

/// Why the person could not be asked.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ConsentError {
    /// Nobody can be asked, so nothing is granted.
    #[error("the person could not be asked: {0}")]
    Unavailable(#[from] ShellError),
}

/// The seam: ask the person whether `app_label` may control the computer with `kinds`.
pub trait Consent: Send + Sync + fmt::Debug {
    /// Ask and wait.
    fn ask<'a>(
        &'a self,
        app_label: &'a str,
        kinds: DeviceKinds,
    ) -> BoxFuture<'a, Result<ConsentAnswer, ConsentError>>;
}

impl<T: Consent + ?Sized> Consent for Arc<T> {
    fn ask<'a>(
        &'a self,
        app_label: &'a str,
        kinds: DeviceKinds,
    ) -> BoxFuture<'a, Result<ConsentAnswer, ConsentError>> {
        (**self).ask(app_label, kinds)
    }
}

/// The question the person sees: which app, and which input it would take over.
pub fn remote_control_question(app_label: &str, kinds: DeviceKinds) -> Question {
    Question::yes_no(
        app_label,
        "Allow control of your computer?",
        format!(
            "{app_label} wants to use your {} to type and click as if it were you. \
             Only allow this for apps you trust.",
            kinds.phrase()
        ),
    )
}

/// The real consent: the shell's yes-or-no sheet. A screen-sharing pick is never a yes; with no
/// shell to ask, nothing is granted.
#[derive(Debug)]
pub struct ShellConsent {
    asker: Box<dyn Asker>,
}

impl ShellConsent {
    /// Consent asked through `asker`.
    pub fn new(asker: Box<dyn Asker>) -> Self {
        Self { asker }
    }
}

impl Consent for ShellConsent {
    fn ask<'a>(
        &'a self,
        app_label: &'a str,
        kinds: DeviceKinds,
    ) -> BoxFuture<'a, Result<ConsentAnswer, ConsentError>> {
        Box::pin(async move {
            let question = remote_control_question(app_label, kinds);
            Ok(match self.asker.ask(&question).await? {
                Answer::Granted => ConsentAnswer::Allowed,
                _ => ConsentAnswer::Declined,
            })
        })
    }
}

/// A consent that answers from a script; when the script is empty the person declines.
#[derive(Debug, Default)]
pub struct FakeConsent {
    answers: Mutex<Vec<ConsentAnswer>>,
    asked: Mutex<Vec<(String, DeviceKinds)>>,
}

impl FakeConsent {
    /// A consent that will give these answers in order.
    pub fn new(answers: Vec<ConsentAnswer>) -> Self {
        let mut answers = answers;
        answers.reverse();
        Self {
            answers: Mutex::new(answers),
            asked: Mutex::new(Vec::new()),
        }
    }

    /// The questions asked so far: the app's label and the kinds.
    pub fn asked(&self) -> Vec<(String, DeviceKinds)> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Consent for FakeConsent {
    fn ask<'a>(
        &'a self,
        app_label: &'a str,
        kinds: DeviceKinds,
    ) -> BoxFuture<'a, Result<ConsentAnswer, ConsentError>> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((app_label.to_owned(), kinds));
        let next = self
            .answers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop()
            .unwrap_or(ConsentAnswer::Declined);
        Box::pin(async move { Ok(next) })
    }
}

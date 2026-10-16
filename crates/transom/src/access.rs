//! `org.freedesktop.impl.portal.Access`: a yes-or-no question about an app.
//!
//! `xdg-desktop-portal` asks it before it takes a screenshot or picks a colour for an app that
//! has not been allowed yet, and refuses to offer those portals when no backend answers it. The
//! question is the person's: the dialog is a surface of the desktop shell, not of this process.
//! [`ShellAsker`] calls `org.quire.Shell1.AskPermission` and [`Answer`] says what came back;
//! [`FakeAsker`] answers from a script, for tests.

use std::sync::{Mutex, PoisonError};

use zbus::zvariant::ObjectPath;

use crate::BoxFuture;
use crate::chooser::{ChooserError, Shell1Proxy, call_failed, nobody_answers};
use crate::link::BusLink;
use crate::names::{AppNames, Layout, tidy_to};
use crate::screencast::Dict;

/// The button that says yes, when the app's request names none.
const GRANT_LABEL: &str = "Allow";
/// The button that says no, when the app's request names none.
const DENY_LABEL: &str = "Don\u{2019}t Allow";

/// The portal's response codes.
const RESPONSE_GRANTED: u32 = 0;
const RESPONSE_DENIED: u32 = 1;
const RESPONSE_DISMISSED: u32 = 1;
const RESPONSE_ENDED: u32 = 2;

/// What the person did with the question.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Answer {
    /// They pressed the yes button.
    Granted,
    /// They pressed the no button.
    Denied,
    /// They closed the question without answering it.
    Dismissed,
}

impl Answer {
    /// The shell's number for an answer: 0 yes, 1 no, 2 dismissed. Any other number is not an
    /// answer.
    pub fn from_wire(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::Granted),
            1 => Some(Self::Denied),
            2 => Some(Self::Dismissed),
            _ => None,
        }
    }

    /// The portal's response code: 0 for a yes, 1 for a no or a question the person closed
    /// (xdg-desktop-portal reads 1 as cancelled by the user; 2 is any other end).
    pub fn response(self) -> u32 {
        match self {
            Self::Granted => RESPONSE_GRANTED,
            Self::Denied => RESPONSE_DENIED,
            Self::Dismissed => RESPONSE_DISMISSED,
        }
    }
}

/// The portal's response code for what asking came to. A shell nobody can reach has been asked
/// nothing and nobody said yes, so that is a no; any other failure ends the question some other
/// way.
pub fn response_of(asked: &Result<Answer, ChooserError>) -> u32 {
    match asked {
        Ok(answer) => answer.response(),
        Err(ChooserError::Unavailable) => {
            tracing::warn!("no shell is available to ask, so the answer is no");
            RESPONSE_DENIED
        }
        Err(error) => {
            tracing::warn!(%error, "the question could not be asked");
            RESPONSE_ENDED
        }
    }
}

/// A question for the person, in the words they see.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Question {
    /// The app's name as the person knows it.
    pub app_label: String,
    /// The headline.
    pub title: String,
    /// The line under the headline.
    pub subtitle: String,
    /// The explanation.
    pub body: String,
    /// The yes button.
    pub grant_label: String,
    /// The no button.
    pub deny_label: String,
}

/// The longest headline taken from an app's request, in characters.
const MAX_TITLE: usize = 128;
/// The longest line under the headline.
const MAX_SUBTITLE: usize = 256;
/// The longest explanation.
const MAX_BODY: usize = 1024;
/// The longest button label.
const MAX_BUTTON: usize = 32;

impl Question {
    /// The question a call carries: the texts as given but cleaned and cut (an app wrote them), the
    /// app by `app_label` (already its own name, see [`AppNames::label`]), and the buttons the
    /// request names (or "Allow" and "Don't Allow").
    pub fn from_call(
        app_label: &str,
        title: &str,
        subtitle: &str,
        body: &str,
        options: &Dict,
    ) -> Self {
        let button = |key: &str, fallback: &str| {
            options
                .get(key)
                .and_then(|v| <&str>::try_from(v).ok())
                .map(|s| tidy_to(s, MAX_BUTTON, Layout::OneLine))
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| fallback.to_owned())
        };
        Self {
            app_label: app_label.to_owned(),
            title: tidy_to(title, MAX_TITLE, Layout::OneLine),
            subtitle: tidy_to(subtitle, MAX_SUBTITLE, Layout::OneLine),
            body: tidy_to(body, MAX_BODY, Layout::Paragraphs),
            grant_label: button("grant_label", GRANT_LABEL),
            deny_label: button("deny_label", DENY_LABEL),
        }
    }
}

/// The seam: show the question and return what the person did.
pub trait Asker: Send + Sync + std::fmt::Debug {
    /// Ask and wait.
    fn ask<'a>(&'a self, question: &'a Question) -> BoxFuture<'a, Result<Answer, ChooserError>>;
}

impl<T: Asker + ?Sized> Asker for std::sync::Arc<T> {
    fn ask<'a>(&'a self, question: &'a Question) -> BoxFuture<'a, Result<Answer, ChooserError>> {
        (**self).ask(question)
    }
}

/// The real asker: the desktop shell's sheet over the session bus, asked from the connection
/// that owns the portal's name.
#[derive(Debug)]
pub struct ShellAsker {
    link: BusLink,
}

impl ShellAsker {
    /// An asker that asks through `link`.
    pub fn new(link: BusLink) -> Self {
        Self { link }
    }
}

impl Asker for ShellAsker {
    fn ask<'a>(&'a self, question: &'a Question) -> BoxFuture<'a, Result<Answer, ChooserError>> {
        Box::pin(async move {
            let connection = self.link.connection()?;
            let proxy = Shell1Proxy::new(&connection).await.map_err(call_failed)?;
            let asked = proxy
                .ask_permission(
                    &question.app_label,
                    &question.title,
                    &question.subtitle,
                    &question.body,
                    &question.grant_label,
                    &question.deny_label,
                )
                .await;
            match asked {
                Ok(value) => Answer::from_wire(value).ok_or(ChooserError::BadAnswer),
                Err(error) if nobody_answers(&error) => Err(ChooserError::Unavailable),
                Err(error) => Err(call_failed(error)),
            }
        })
    }
}

/// An asker with nobody behind it: every question is unanswerable, which reads as a no. What a
/// service nobody gave a shell to does.
#[derive(Debug, Default)]
pub struct NoAsker;

impl Asker for NoAsker {
    fn ask<'a>(&'a self, _: &'a Question) -> BoxFuture<'a, Result<Answer, ChooserError>> {
        Box::pin(async { Err(ChooserError::Unavailable) })
    }
}

/// An asker that answers from a script, for tests: each call takes the next answer; when the
/// script is empty the person denies.
#[derive(Debug, Default)]
pub struct FakeAsker {
    answers: Mutex<Vec<Answer>>,
    asked: Mutex<Vec<Question>>,
}

impl FakeAsker {
    /// An asker that will give these answers in order.
    pub fn new(answers: Vec<Answer>) -> Self {
        let mut answers = answers;
        answers.reverse();
        Self {
            answers: Mutex::new(answers),
            asked: Mutex::new(Vec::new()),
        }
    }

    /// The questions asked so far.
    pub fn asked(&self) -> Vec<Question> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Asker for FakeAsker {
    fn ask<'a>(&'a self, question: &'a Question) -> BoxFuture<'a, Result<Answer, ChooserError>> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(question.clone());
        let next = self
            .answers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop()
            .unwrap_or(Answer::Denied);
        Box::pin(async move { Ok(next) })
    }
}

/// The served interface.
#[derive(Debug)]
pub struct AccessService {
    asker: Box<dyn Asker>,
    names: AppNames,
}

impl AccessService {
    /// A service whose questions go to `asker`; apps are shown by their ids until
    /// [`AccessService::with_names`] says where their names are.
    pub fn new(asker: Box<dyn Asker>) -> Self {
        Self {
            asker,
            names: AppNames::none(),
        }
    }

    /// This service showing apps by the names `names` finds.
    pub fn with_names(mut self, names: AppNames) -> Self {
        self.names = names;
        self
    }

    /// Ask the question a call carries and give the reply: the response code and no results.
    pub async fn decide(&self, question: &Question) -> (u32, Dict) {
        (response_of(&self.asker.ask(question).await), Dict::new())
    }
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Access")]
impl AccessService {
    /// Ask the person a yes-or-no question about an app. The `icon` and `choices` options are
    /// not used: the shell shows the app by its own icon and the question has no extra choices.
    #[allow(clippy::too_many_arguments)] // the portal's own signature
    async fn access_dialog(
        &self,
        handle: ObjectPath<'_>,
        app_id: &str,
        parent_window: &str,
        title: &str,
        subtitle: &str,
        body: &str,
        options: Dict,
    ) -> (u32, Dict) {
        let _ = (handle, parent_window);
        let question =
            Question::from_call(&self.names.label(app_id), title, subtitle, body, &options);
        self.decide(&question).await
    }

    /// The interface version.
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        1
    }
}

//! The shell the portal asks: only the program the caller table names as the shell is asked, the
//! owner is looked up again for every question, and a question ends as a no when the shell
//! changes while it is open. Stand-ins on a private bus, introduced with the standing they would have.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use peephole_bus::peephole::fake::introduced_program;
use transom::access::{Answer, Asker, Question, ShellAsker, response_of};
use transom::chooser::ChooserError;
use transom::shell::SHELL_NAME;
use zbus::Connection;
use zbus::zvariant::OwnedValue;

use crate::guard::{Portal, as_shell, introduce, start_portal};

const SETTLE: Duration = Duration::from_secs(5);

/// A stand-in shell: counts the questions it is asked and answers yes, or never answers.
struct StandIn {
    asked: Arc<AtomicUsize>,
    answers: Answers,
}

#[derive(Clone, Copy)]
enum Answers {
    Yes,
    Never,
}

#[zbus::interface(name = "org.quire.Shell1")]
impl StandIn {
    #[allow(clippy::too_many_arguments)] // the shell's own signature
    async fn ask_permission(
        &self,
        _app_label: String,
        _title: String,
        _subtitle: String,
        _body: String,
        _grant_label: String,
        _deny_label: String,
    ) -> u32 {
        self.asked.fetch_add(1, Ordering::SeqCst);
        match self.answers {
            Answers::Yes => 0,
            Answers::Never => std::future::pending().await,
        }
    }
}

async fn stand_in(portal: &Portal, answers: Answers) -> (Connection, Arc<AtomicUsize>) {
    let asked = Arc::new(AtomicUsize::new(0));
    let connection = portal
        .bus
        .builder()
        .name(SHELL_NAME)
        .and_then(|b| {
            b.serve_at(
                "/org/quire/Shell1",
                StandIn {
                    asked: Arc::clone(&asked),
                    answers,
                },
            )
        })
        .expect("the stand-in shell")
        .build()
        .await
        .expect("the stand-in shell");
    (connection, asked)
}

fn question() -> Question {
    Question::from_call(
        "Meet",
        "Allow?",
        "",
        "",
        &std::collections::HashMap::<String, OwnedValue>::new(),
    )
}

async fn ask(asker: &ShellAsker) -> Result<Answer, ChooserError> {
    tokio::time::timeout(SETTLE, asker.ask(&question()))
        .await
        .expect("answered in time")
}

#[tokio::test(flavor = "multi_thread")]
async fn only_the_shell_is_asked_and_a_squatter_is_not() {
    let Some(portal) = start_portal().await else {
        return;
    };
    let asker = ShellAsker::new(portal.link.clone());

    // Nobody owns the shell's name: nobody is asked, and that is a no.
    let nobody = ask(&asker).await;
    assert!(
        nobody.as_ref().is_err_and(ChooserError::is_refusal),
        "{nobody:?}"
    );

    // A squatter that is no shell owns the name: it is never asked.
    let (squatter, asked_squatter) = stand_in(&portal, Answers::Yes).await;
    introduce(
        &portal.peers,
        &squatter,
        introduced_program("/usr/bin/evil"),
    );
    let refused = ask(&asker).await;
    assert!(
        matches!(refused, Err(ChooserError::NotTheShell)),
        "{refused:?}"
    );
    assert_eq!(
        asked_squatter.load(Ordering::SeqCst),
        0,
        "a squatter hears nothing"
    );
    squatter
        .release_name(SHELL_NAME)
        .await
        .expect("the squatter leaves");

    // The real shell takes the name: the control. Each question looks the owner up again.
    let (shell, asked_shell) = stand_in(&portal, Answers::Yes).await;
    as_shell(&portal.peers, &shell);
    assert_eq!(
        ask(&asker).await.expect("the shell answers"),
        Answer::Granted
    );
    shell
        .release_name(SHELL_NAME)
        .await
        .expect("the shell leaves");
    let (squatter, asked_again) = stand_in(&portal, Answers::Yes).await;
    introduce(
        &portal.peers,
        &squatter,
        introduced_program("/usr/bin/evil"),
    );
    let after_restart = ask(&asker).await;
    assert!(
        matches!(after_restart, Err(ChooserError::NotTheShell)),
        "{after_restart:?}"
    );
    assert_eq!(asked_shell.load(Ordering::SeqCst), 1);
    assert_eq!(asked_again.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_question_ends_as_a_no_when_the_shell_changes_while_it_is_open() {
    let Some(portal) = start_portal().await else {
        return;
    };
    let asker = Arc::new(ShellAsker::new(portal.link.clone()));

    let (shell, asked) = stand_in(&portal, Answers::Never).await;
    as_shell(&portal.peers, &shell);
    let open = {
        let asker = Arc::clone(&asker);
        tokio::spawn(async move { ask(&asker).await })
    };
    // Wait until the shell has the question in front of it, then let it go.
    for _ in 0..100 {
        if asked.load(Ordering::SeqCst) == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        asked.load(Ordering::SeqCst),
        1,
        "the question reached the shell"
    );
    shell
        .release_name(SHELL_NAME)
        .await
        .expect("the shell lets go of its name");

    let ended = open.await.expect("the question task");
    assert!(
        matches!(ended, Err(ChooserError::ShellChanged)),
        "{ended:?}"
    );
    assert_eq!(response_of(&ended), 1, "an ended question is a no");
}

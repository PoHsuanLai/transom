use transom::access::{AccessService, Answer, FakeAsker, Question, response_of};
use transom::chooser::{
    Chooser, FakeChooser, ShellError, answer_from_wire, nobody_answers, source_from_wire,
    unavailable_is_cancel,
};
use transom::names::AppNames;
use transom::screencast::Dict;
use transom_core::{CastSource, ChoiceRequest, CursorMode, Multiple, SourceKinds};
use zbus::zvariant::{OwnedValue, Value};

fn request() -> ChoiceRequest {
    ChoiceRequest {
        kinds: SourceKinds::BOTH,
        multiple: Multiple::One,
        cursor_modes: vec![CursorMode::Embedded],
        app_label: "Meet".to_owned(),
    }
}

#[test]
fn wire_sources_round_trip_and_unknown_kinds_are_refused() {
    for source in [
        CastSource::Output("A".into()),
        CastSource::Window("w".into()),
    ] {
        let (kind, name) = source.wire();
        assert_eq!(source_from_wire(kind, name), Some(source));
    }
    assert_eq!(source_from_wire("region", "x"), None);
}

#[tokio::test]
async fn the_fake_answers_in_order_then_cancels() {
    let ok = (vec![CastSource::Output("A".into())], CursorMode::Embedded);
    let bad_cursor = (vec![CastSource::Output("A".into())], CursorMode::Hidden);
    let chooser = FakeChooser::new(vec![Some(ok), Some(bad_cursor)]);
    let req = request();
    let first = chooser.pick(&req).await.expect("answer");
    assert_eq!(first.map(|c| c.sources.len()), Some(1));
    assert!(
        chooser.pick(&req).await.expect("answer").is_none(),
        "an unacceptable answer is a cancel"
    );
    assert!(
        chooser.pick(&req).await.expect("answer").is_none(),
        "an empty script is a cancel"
    );
}

#[test]
fn the_shells_answer_becomes_a_choice_a_cancel_or_a_bad_answer() {
    let window = ("window".to_owned(), "w1".to_owned());
    let output = ("output".to_owned(), "DP-1".to_owned());

    let choice = answer_from_wire(&[output.clone(), window.clone()], 2)
        .expect("an answer")
        .expect("a choice");
    assert_eq!(
        choice.sources,
        vec![
            CastSource::Output("DP-1".into()),
            CastSource::Window("w1".into())
        ]
    );
    assert_eq!(choice.cursor, CursorMode::Embedded);

    assert!(answer_from_wire(&[], 1).expect("a cancel").is_none());
    for (sources, cursor) in [
        (vec![("tab".to_owned(), "x".to_owned())], 1),
        (vec![window.clone()], 3),
        (vec![window], 0),
    ] {
        assert!(matches!(
            answer_from_wire(&sources, cursor),
            Err(ShellError::BadAnswer)
        ));
    }
}

#[test]
fn a_shell_that_cannot_answer_is_a_cancel_and_other_failures_stay_failures() {
    use zbus::fdo::Error as Fdo;
    let boxed = |e: Fdo| zbus::Error::FDO(Box::new(e));
    for (error, nobody) in [
        (boxed(Fdo::ServiceUnknown("x".into())), true),
        (boxed(Fdo::NameHasNoOwner("x".into())), true),
        (boxed(Fdo::UnknownObject("x".into())), true),
        (boxed(Fdo::UnknownInterface("x".into())), true),
        (boxed(Fdo::UnknownMethod("x".into())), true),
        (boxed(Fdo::AccessDenied("x".into())), false),
        (boxed(Fdo::LimitsExceeded("x".into())), false),
        (zbus::Error::Unsupported, false),
    ] {
        assert_eq!(nobody_answers(&error), nobody, "{error}");
    }
    for refusal in [
        ShellError::Unavailable,
        ShellError::NotTheShell,
        ShellError::ShellChanged,
    ] {
        assert!(matches!(unavailable_is_cancel(Err(refusal)), Ok(None)));
    }
    for failure in [ShellError::BadAnswer, ShellError::Call("x".into())] {
        assert!(unavailable_is_cancel(Err(failure)).is_err());
    }
    assert!(matches!(unavailable_is_cancel(Ok(None)), Ok(None)));
}

#[test]
fn what_asking_came_to_is_the_portals_response_code() {
    let cases: [(&str, Result<Answer, ShellError>, u32); 8] = [
        ("granted", Ok(Answer::Granted), 0),
        ("denied", Ok(Answer::Denied), 1),
        ("dismissed is cancelled", Ok(Answer::Dismissed), 1),
        ("no shell is a no", Err(ShellError::Unavailable), 1),
        ("not the shell is a no", Err(ShellError::NotTheShell), 1),
        ("a replaced shell is a no", Err(ShellError::ShellChanged), 1),
        ("a bad answer ends it", Err(ShellError::BadAnswer), 2),
        (
            "a failed call ends it",
            Err(ShellError::Call("x".into())),
            2,
        ),
    ];
    for (name, asked, want) in cases {
        assert_eq!(response_of(&asked), want, "{name}");
    }
    for (wire, want) in [
        (0, Some(Answer::Granted)),
        (1, Some(Answer::Denied)),
        (2, Some(Answer::Dismissed)),
        (3, None),
    ] {
        assert_eq!(Answer::from_wire(wire), want, "shell answer {wire}");
    }
}

fn text(value: &str) -> OwnedValue {
    OwnedValue::try_from(Value::new(value)).expect("a string variant")
}

#[tokio::test]
async fn the_access_dialog_asks_the_shell_in_the_apps_words_and_replies_with_its_answer() {
    let asker = std::sync::Arc::new(FakeAsker::new(vec![
        Answer::Granted,
        Answer::Denied,
        Answer::Dismissed,
    ]));
    let service = AccessService::new(Box::new(asker.clone()));
    let mut options = Dict::new();
    options.insert("grant_label".to_owned(), text("Share"));
    options.insert("deny_label".to_owned(), text(" "));
    let question = Question::from_call(
        &AppNames::none().label("org.example.screen-grab.desktop"),
        "Take a screenshot?",
        "",
        "It can see everything.",
        &options,
    );
    assert_eq!(question.app_label, "org.example.screen-grab");
    assert_eq!(question.grant_label, "Share");
    assert_eq!(
        question.deny_label, "Don\u{2019}t Allow",
        "a blank label falls back"
    );
    for want in [0, 1, 1, 1] {
        let (code, results) = service.decide(&question).await;
        assert_eq!(code, want);
        assert!(results.is_empty());
    }
    assert_eq!(asker.asked().len(), 4);
    assert_eq!(asker.asked()[0], question);
}

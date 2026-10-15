use transom::chooser::{
    Chooser, ChooserError, FakeChooser, answer_from_wire, nobody_answers, source_from_wire,
    source_to_wire, unavailable_is_cancel,
};
use transom_core::{CastSource, ChoiceRequest, CursorMode, Multiple, SourceKinds};

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
        let (kind, name) = source_to_wire(&source);
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
            Err(ChooserError::BadAnswer)
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
    assert!(matches!(
        unavailable_is_cancel(Err(ChooserError::Unavailable)),
        Ok(None)
    ));
    for failure in [ChooserError::BadAnswer, ChooserError::Call("x".into())] {
        assert!(unavailable_is_cancel(Err(failure)).is_err());
    }
    assert!(matches!(unavailable_is_cancel(Ok(None)), Ok(None)));
}

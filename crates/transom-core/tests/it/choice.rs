use transom_core::choice::{ChoiceRefusal, ChoiceRequest, Multiple, SourceKinds};
use transom_core::{CastSource, CursorMode};

fn out(name: &str) -> CastSource {
    CastSource::Output(name.to_owned())
}

fn win(id: &str) -> CastSource {
    CastSource::Window(id.to_owned())
}

fn request(kinds: SourceKinds, multiple: Multiple) -> ChoiceRequest {
    ChoiceRequest {
        kinds,
        multiple,
        cursor_modes: vec![CursorMode::Hidden, CursorMode::Embedded],
        app_label: "Meet".to_owned(),
    }
}

#[test]
fn kinds_from_the_wire() {
    assert_eq!(SourceKinds::from_wire(1), SourceKinds::OUTPUTS);
    assert_eq!(SourceKinds::from_wire(2), SourceKinds::WINDOWS);
    assert_eq!(SourceKinds::from_wire(3), SourceKinds::BOTH);
    assert_eq!(SourceKinds::from_wire(0), SourceKinds::OUTPUTS);
    assert_eq!(
        SourceKinds::from_wire(4),
        SourceKinds::OUTPUTS,
        "virtual only is not served: displays"
    );
    assert_eq!(SourceKinds::from_wire(7), SourceKinds::BOTH);
}

#[test]
fn the_acceptance_table() {
    let table = [
        (
            "one display",
            SourceKinds::BOTH,
            Multiple::One,
            vec![out("A")],
            CursorMode::Embedded,
            Ok(()),
        ),
        (
            "two when many",
            SourceKinds::BOTH,
            Multiple::Many,
            vec![out("A"), win("w")],
            CursorMode::Hidden,
            Ok(()),
        ),
        (
            "two when one",
            SourceKinds::BOTH,
            Multiple::One,
            vec![out("A"), out("B")],
            CursorMode::Hidden,
            Err(ChoiceRefusal::TooMany),
        ),
        (
            "nothing",
            SourceKinds::BOTH,
            Multiple::One,
            vec![],
            CursorMode::Hidden,
            Err(ChoiceRefusal::Empty),
        ),
        (
            "a window when only displays",
            SourceKinds::OUTPUTS,
            Multiple::One,
            vec![win("w")],
            CursorMode::Hidden,
            Err(ChoiceRefusal::WrongKind),
        ),
        (
            "a display when only windows",
            SourceKinds::WINDOWS,
            Multiple::One,
            vec![out("A")],
            CursorMode::Hidden,
            Err(ChoiceRefusal::WrongKind),
        ),
        (
            "the same twice",
            SourceKinds::BOTH,
            Multiple::Many,
            vec![out("A"), out("A")],
            CursorMode::Hidden,
            Err(ChoiceRefusal::Duplicate),
        ),
        (
            "a cursor the app did not allow",
            SourceKinds::BOTH,
            Multiple::One,
            vec![out("A")],
            CursorMode::Metadata,
            Err(ChoiceRefusal::CursorNotAllowed),
        ),
    ];
    for (name, kinds, multiple, sources, cursor, want) in table {
        let got = request(kinds, multiple).accept(sources, cursor).map(|_| ());
        assert_eq!(got, want, "{name}");
    }
}

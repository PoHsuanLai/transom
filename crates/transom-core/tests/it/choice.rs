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
    assert_eq!(SourceKinds::from_wire(1), SourceKinds::Outputs);
    assert_eq!(SourceKinds::from_wire(2), SourceKinds::Windows);
    assert_eq!(SourceKinds::from_wire(3), SourceKinds::Both);
    assert_eq!(SourceKinds::from_wire(0), SourceKinds::Outputs);
    assert_eq!(
        SourceKinds::from_wire(4),
        SourceKinds::Outputs,
        "virtual only is not served: displays"
    );
    assert_eq!(SourceKinds::from_wire(7), SourceKinds::Both);
}

#[test]
fn the_acceptance_table() {
    let table = [
        (
            "one display",
            SourceKinds::Both,
            Multiple::One,
            vec![out("A")],
            CursorMode::Embedded,
            Ok(()),
        ),
        (
            "two when many",
            SourceKinds::Both,
            Multiple::Many,
            vec![out("A"), win("w")],
            CursorMode::Hidden,
            Ok(()),
        ),
        (
            "two when one",
            SourceKinds::Both,
            Multiple::One,
            vec![out("A"), out("B")],
            CursorMode::Hidden,
            Err(ChoiceRefusal::TooMany),
        ),
        (
            "nothing",
            SourceKinds::Both,
            Multiple::One,
            vec![],
            CursorMode::Hidden,
            Err(ChoiceRefusal::Empty),
        ),
        (
            "a window when only displays",
            SourceKinds::Outputs,
            Multiple::One,
            vec![win("w")],
            CursorMode::Hidden,
            Err(ChoiceRefusal::WrongKind),
        ),
        (
            "a display when only windows",
            SourceKinds::Windows,
            Multiple::One,
            vec![out("A")],
            CursorMode::Hidden,
            Err(ChoiceRefusal::WrongKind),
        ),
        (
            "the same twice",
            SourceKinds::Both,
            Multiple::Many,
            vec![out("A"), out("A")],
            CursorMode::Hidden,
            Err(ChoiceRefusal::Duplicate),
        ),
        (
            "a cursor the app did not allow",
            SourceKinds::Both,
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

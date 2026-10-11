use proptest::prelude::*;
use transom_core::restore::Grant;
use transom_core::{CastSource, CursorMode, PersistMode, RestoreBook, RestoreToken};

fn out(name: &str) -> CastSource {
    CastSource::Output(name.to_owned())
}

#[test]
fn a_token_is_32_hex_digits_and_its_debug_hides_it() {
    let token = RestoreToken::mint([0xab; 16]);
    assert_eq!(token.expose(), "abababababababababababababababab");
    let shown = format!("{token:?}");
    assert!(!shown.contains("abab"), "{shown}");
}

#[test]
fn parse_accepts_only_what_mint_makes() {
    for (text, want) in [
        ("0123456789abcdef0123456789abcdef", true),
        ("0123456789ABCDEF0123456789ABCDEF", false),
        ("0123", false),
        ("", false),
        ("0123456789abcdef0123456789abcdeg", false),
    ] {
        assert_eq!(RestoreToken::parse(text).is_some(), want, "{text:?}");
    }
}

proptest! {
    #[test]
    fn mint_then_parse_round_trips(bytes in any::<[u8; 16]>()) {
        let token = RestoreToken::mint(bytes);
        prop_assert_eq!(RestoreToken::parse(token.expose()), Some(token));
    }
}

#[test]
fn resolve_is_all_or_nothing_and_per_app() {
    let token = RestoreToken::mint([1; 16]);
    let mut book = RestoreBook::new();
    book.remember(
        token.clone(),
        Grant {
            app_id: "org.example.Meet".into(),
            sources: vec![out("A"), out("B")],
        },
    );
    let live_all = [out("A"), out("B"), out("C")];
    let live_less = [out("A"), out("C")];
    let table: [(&str, &str, &[CastSource], usize); 4] = [
        ("right app, all live", "org.example.Meet", &live_all, 2),
        ("right app, one gone", "org.example.Meet", &live_less, 0),
        ("another app", "org.example.Other", &live_all, 0),
        ("no sources live", "org.example.Meet", &[], 0),
    ];
    for (name, app, live, want) in table {
        assert_eq!(book.resolve(&token, app, live).len(), want, "{name}");
    }
    let unknown = RestoreToken::mint([2; 16]);
    assert!(
        book.resolve(&unknown, "org.example.Meet", &live_all)
            .is_empty()
    );
    assert!(book.forget(&token).is_some());
    assert!(book.is_empty());
}

#[test]
fn wire_values_round_trip() {
    for mode in [
        PersistMode::DoNot,
        PersistMode::WhileRunning,
        PersistMode::UntilRevoked,
    ] {
        assert_eq!(PersistMode::from_wire(mode.to_wire()), Some(mode));
    }
    assert_eq!(PersistMode::from_wire(3), None);
    for mode in [
        CursorMode::Hidden,
        CursorMode::Embedded,
        CursorMode::Metadata,
    ] {
        assert_eq!(CursorMode::from_wire(mode.to_wire()), Some(mode));
    }
    assert_eq!(CursorMode::from_wire(3), None);
    assert_eq!(CursorMode::from_wire(0), None);
}

use proptest::prelude::*;
use transom_core::restore::Grant;
use transom_core::{
    CastSource, CursorMode, MAX_GRANTS, PersistMode, RestoreBook, RestoreToken, UNTIL_REVOKED_SECS,
    WHILE_RUNNING_SECS,
};

const NOW: u64 = 1_800_000_000;

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

fn grant(app: &str, mode: PersistMode, at: u64) -> Grant {
    Grant::issue(app, vec![out("A"), out("B")], mode, at).expect("a grant")
}

#[test]
fn resolve_is_all_or_nothing_and_per_app() {
    let token = RestoreToken::mint([1; 16]);
    let mut book = RestoreBook::new();
    book.remember(
        token.clone(),
        grant("org.example.Meet", PersistMode::UntilRevoked, NOW),
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
        assert_eq!(book.resolve(&token, app, live, NOW).len(), want, "{name}");
    }
    let unknown = RestoreToken::mint([2; 16]);
    assert!(
        book.resolve(&unknown, "org.example.Meet", &live_all, NOW)
            .is_empty()
    );
    assert!(book.forget(&token).is_some());
    assert!(book.is_empty());
}

#[test]
fn a_grant_expires_by_its_mode_and_a_future_date_is_not_trusted() {
    let live = [out("A"), out("B")];
    let table = [
        (PersistMode::WhileRunning, NOW - 60, true),
        (PersistMode::WhileRunning, NOW - WHILE_RUNNING_SECS, true),
        (
            PersistMode::WhileRunning,
            NOW - WHILE_RUNNING_SECS - 1,
            false,
        ),
        (
            PersistMode::UntilRevoked,
            NOW - WHILE_RUNNING_SECS - 1,
            true,
        ),
        (PersistMode::UntilRevoked, NOW - UNTIL_REVOKED_SECS, true),
        (
            PersistMode::UntilRevoked,
            NOW - UNTIL_REVOKED_SECS - 1,
            false,
        ),
        (PersistMode::UntilRevoked, NOW + 1, false),
    ];
    for (mode, at, valid) in table {
        let token = RestoreToken::mint([3; 16]);
        let mut book = RestoreBook::new();
        book.remember(token.clone(), grant("org.example.Meet", mode, at));
        let got = book.resolve(&token, "org.example.Meet", &live, NOW);
        assert_eq!(!got.is_empty(), valid, "{mode:?} granted at {at}");
    }
}

#[test]
fn an_app_with_no_identity_is_never_remembered() {
    // Every app that arrives with an empty id would share one grant: ask each time instead.
    assert!(Grant::issue("", vec![out("A")], PersistMode::UntilRevoked, NOW).is_none());
    assert!(Grant::issue("org.example.Meet", vec![out("A")], PersistMode::DoNot, NOW).is_none());
    assert!(Grant::issue("org.example.Meet", vec![], PersistMode::UntilRevoked, NOW).is_none());
    // Even a grant that somehow sits in the book under an empty id does not resolve for one.
    let token = RestoreToken::mint([4; 16]);
    let mut book = RestoreBook::new();
    book.remember(
        token.clone(),
        Grant {
            app_id: String::new(),
            sources: vec![out("A")],
            mode: PersistMode::UntilRevoked,
            granted_at: NOW,
        },
    );
    assert!(book.resolve(&token, "", &[out("A")], NOW).is_empty());
}

#[test]
fn pruning_drops_the_expired_and_then_the_oldest_past_the_cap() {
    let mut book = RestoreBook::new();
    book.remember(
        RestoreToken::mint([0xee; 16]),
        grant(
            "old",
            PersistMode::UntilRevoked,
            NOW - UNTIL_REVOKED_SECS - 5,
        ),
    );
    for n in 0..(MAX_GRANTS + 3) {
        let mut bytes = [0_u8; 16];
        bytes[..8].copy_from_slice(&(n as u64).to_be_bytes());
        book.remember(
            RestoreToken::mint(bytes),
            grant("app", PersistMode::UntilRevoked, NOW - 1000 + n as u64),
        );
    }
    assert_eq!(book.prune(NOW), 1 + 3, "one expired, three over the cap");
    assert_eq!(book.len(), MAX_GRANTS);
    let oldest_kept = book.iter().map(|(_, g)| g.granted_at).min();
    assert_eq!(oldest_kept, Some(NOW - 1000 + 3), "the oldest went first");
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

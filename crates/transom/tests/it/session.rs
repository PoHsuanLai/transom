use std::time::{Duration, Instant};
use transom::screencast::restore::{MemoryStore, RestoreStore};

use transom::peers::{SESSION_LIMITS, Tether, UNSTARTED_TTL};
use transom::screencast::session::{Refusal, SessionBook, SessionKey};
use transom_core::restore::Grant;
use transom_core::{CastIn, CastOut, CastSource, PwNode, RestoreBook, RestoreToken};

fn key(n: u32) -> SessionKey {
    SessionKey(format!("/org/freedesktop/portal/desktop/session/x/{n}"))
}

#[test]
fn a_session_runs_to_its_end_and_leaves_the_book() {
    let mut book = SessionBook::new();
    let now = Instant::now();
    assert!(
        book.create(key(1), "org.example.Meet", Tether::default(), now)
            .is_ok()
    );
    assert_eq!(
        book.create(key(1), "org.example.Meet", Tether::default(), now),
        Err(Refusal::AlreadyOpen),
        "a handle is opened once"
    );
    let src = CastSource::Window("w1".to_owned());

    assert_eq!(
        book.apply(&key(1), CastIn::Start { restored: vec![] }),
        vec![CastOut::AskChooser]
    );
    assert_eq!(
        book.apply(&key(1), CastIn::Picked(vec![src.clone()])),
        vec![CastOut::OpenStream(vec![src])]
    );
    book.apply(&key(1), CastIn::NodeReady(PwNode(3)));
    assert_eq!(book.len(), 1);
    assert_eq!(
        book.apply(&key(1), CastIn::Close),
        vec![CastOut::CloseStream]
    );
    assert!(book.is_empty());
    assert!(
        book.apply(&key(1), CastIn::Close).is_empty(),
        "an unknown session does nothing"
    );
}

#[test]
fn the_memory_store_keeps_what_it_is_given() {
    let store = MemoryStore::default();
    assert!(store.load().expect("load").is_empty());
    let mut book = RestoreBook::new();
    book.remember(
        RestoreToken::mint([5; 16]),
        Grant {
            app_id: "org.example.Meet".into(),
            sources: vec![CastSource::Output("A".into())],
        },
    );
    store.save(&book).expect("save");
    assert_eq!(store.load().expect("load"), book);
}

fn caller(n: u32) -> Tether {
    Tether::new(Some(format!(":1.{n}")), Some(":1.1".to_owned()))
}

#[test]
fn one_caller_and_the_whole_book_are_capped() {
    let now = Instant::now();
    let mut book = SessionBook::new();
    for n in 0..SESSION_LIMITS.per_caller {
        let n = u32::try_from(n).expect("small");
        assert!(book.create(key(n), "a", caller(7), now).is_ok(), "{n}");
    }
    assert_eq!(
        book.create(key(1000), "a", caller(7), now),
        Err(Refusal::Full),
        "one caller cannot fill the book"
    );
    assert!(
        book.create(key(1001), "a", caller(8), now).is_ok(),
        "another caller still fits"
    );
    let mut many = SessionBook::new();
    for n in 0..SESSION_LIMITS.total {
        let n = u32::try_from(n).expect("small");
        assert!(many.create(key(n), "a", caller(100 + n), now).is_ok());
    }
    assert_eq!(
        many.create(key(5000), "a", caller(5000), now),
        Err(Refusal::Full)
    );
}

#[test]
fn a_full_book_drops_sessions_that_never_began_but_not_a_live_share() {
    let start = Instant::now();
    let mut book = SessionBook::new();
    for n in 0..SESSION_LIMITS.per_caller {
        let n = u32::try_from(n).expect("small");
        book.create(key(n), "a", caller(7), start).expect("room");
    }
    // Session 0 is a share that is running.
    let src = CastSource::Output("DP-1".to_owned());
    book.apply(&key(0), CastIn::Start { restored: vec![] });
    book.apply(&key(0), CastIn::Picked(vec![src]));
    book.apply(&key(0), CastIn::NodeReady(PwNode(3)));

    let soon = start + Duration::from_secs(60);
    assert_eq!(
        book.create(key(900), "a", caller(7), soon),
        Err(Refusal::Full),
        "too young to drop"
    );
    let later = start + UNSTARTED_TTL + Duration::from_secs(1);
    let dropped = book
        .create(key(901), "a", caller(7), later)
        .expect("room was made");
    assert_eq!(dropped.len(), SESSION_LIMITS.per_caller - 1);
    assert!(!dropped.contains(&key(0)), "a live share is never dropped");
    assert!(book.get(&key(0)).is_some());
}

#[test]
fn a_session_hangs_on_its_caller_and_on_the_frontend() {
    let now = Instant::now();
    let mut book = SessionBook::new();
    book.create(
        key(1),
        "a",
        Tether::new(Some(":1.7".into()), Some(":1.2".into())),
        now,
    )
    .expect("room");
    book.create(
        key(2),
        "a",
        Tether::new(Some(":1.8".into()), Some(":1.2".into())),
        now,
    )
    .expect("room");
    assert_eq!(book.held_by(":1.7"), vec![key(1)]);
    assert_eq!(book.held_by(":1.2"), vec![key(1), key(2)]);
    assert!(book.held_by(":1.99").is_empty());
}

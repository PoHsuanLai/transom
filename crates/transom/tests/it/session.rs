use transom::screencast::restore::{MemoryStore, RestoreStore};
use transom::screencast::session::{SessionBook, SessionKey};
use transom_core::restore::Grant;
use transom_core::{CastIn, CastOut, CastSource, PwNode, RestoreBook, RestoreToken};

fn key(n: u32) -> SessionKey {
    SessionKey(format!("/org/freedesktop/portal/desktop/session/x/{n}"))
}

#[test]
fn a_session_runs_to_its_end_and_leaves_the_book() {
    let mut book = SessionBook::new();
    assert!(book.create(key(1), "org.example.Meet").is_some());
    assert!(
        book.create(key(1), "org.example.Meet").is_none(),
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

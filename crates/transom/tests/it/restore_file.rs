use std::os::unix::fs::PermissionsExt;

use transom::screencast::restore::{FileStore, RestoreStore, StoreError};
use transom_core::restore::Grant;
use transom_core::{CastSource, RestoreBook, RestoreToken};

fn book_of_two() -> RestoreBook {
    let mut book = RestoreBook::new();
    book.remember(
        RestoreToken::mint([1; 16]),
        Grant {
            app_id: "org.example.Meet".into(),
            sources: vec![CastSource::Output("DP-1".into())],
        },
    );
    book.remember(
        RestoreToken::mint([2; 16]),
        Grant {
            app_id: "back\\slash\nand\rtabs\t".into(),
            sources: vec![
                CastSource::Window("a:b\\n".into()),
                CastSource::Window(String::new()),
            ],
        },
    );
    book
}

#[test]
fn awkward_text_survives_a_save_and_a_load() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FileStore::new(dir.path().join("deeper").join("restore"));
    let book = book_of_two();
    store.save(&book).expect("save creates the directory");
    assert_eq!(store.load().expect("load"), book);
    store.save(&RestoreBook::new()).expect("save an empty book");
    assert!(store.load().expect("load").is_empty());
}

#[test]
fn the_file_is_private_to_its_owner() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = FileStore::new(dir.path().join("restore"));
    store.save(&book_of_two()).expect("save");
    let mode = std::fs::metadata(store.path())
        .expect("metadata")
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn a_file_this_version_did_not_write_is_damaged_not_empty() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("restore");
    let token = RestoreToken::mint([4; 16]);
    let token = token.expose();
    for text in [
        String::new(),
        "something else\n".to_owned(),
        "transom-restore 1\nnot-a-token\tapp\n".to_owned(),
        format!("transom-restore 1\n{token}\n"),
        format!("transom-restore 1\n{token}\tapp\ttab:x\n"),
        format!("transom-restore 1\n{token}\tapp\toutput:bad\\q\n"),
    ] {
        std::fs::write(&path, &text).expect("write");
        assert!(
            matches!(
                FileStore::new(path.clone()).load(),
                Err(StoreError::Damaged)
            ),
            "text {text:?}"
        );
    }
}

use std::os::unix::fs::PermissionsExt;

use transom::screencast::restore::{FileStore, RestoreStore, StoreError};
use transom_core::restore::Grant;
use transom_core::{CastSource, PersistMode, RestoreBook, RestoreToken};

fn book_of_two() -> RestoreBook {
    let mut book = RestoreBook::new();
    book.remember(
        RestoreToken::mint([1; 16]),
        Grant {
            app_id: "org.example.Meet".into(),
            sources: vec![CastSource::Output("DP-1".into())],
            mode: PersistMode::UntilRevoked,
            granted_at: 1_800_000_000,
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
            mode: PersistMode::WhileRunning,
            granted_at: 1_800_000_123,
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

const HEADER: &str = "transom-restore 2";

#[test]
fn a_file_this_version_did_not_write_is_set_aside_and_reported_damaged() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("restore");
    for text in [
        String::new(),
        "something else\n".to_owned(),
        "transom-restore 3\n".to_owned(),
    ] {
        std::fs::write(&path, &text).expect("write");
        let store = FileStore::new(path.clone());
        assert!(
            matches!(store.load(), Err(StoreError::Damaged)),
            "text {text:?}"
        );
        assert!(
            !path.exists(),
            "the foreign file is moved away, not overwritten later"
        );
        assert_eq!(
            std::fs::read_to_string(store.bad_path()).expect("kept"),
            text
        );
    }
}

#[test]
fn a_flawed_line_loses_that_grant_only_and_the_file_is_kept_beside() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("restore");
    let good = RestoreToken::mint([4; 16]);
    let good = good.expose();
    let text = format!(
        "{HEADER}\nnot-a-token\tapp\t2\t1\n{good}\tapp\t2\t1800000000\toutput:DP-1\n\
         {good}\n{good}\tapp\t9\t1\tdisplay:x\n{good}\tapp\t2\tsoon\n{good}\tapp\t2\t1\ttab:x\n\
         {good}\tapp\t2\t1\toutput:bad\\q\n"
    );
    std::fs::write(&path, &text).expect("write");
    let store = FileStore::new(path.clone());
    let book = store.load().expect("the readable grants are kept");
    assert_eq!(book.len(), 1);
    let token = RestoreToken::parse(good).expect("token");
    let live = [CastSource::Output("DP-1".into())];
    assert_eq!(book.resolve(&token, "app", &live, 1_800_000_001).len(), 1);
    assert_eq!(
        std::fs::read_to_string(store.bad_path()).expect("a copy is kept"),
        text
    );
    assert!(path.exists(), "the file itself stays");
}

#[test]
fn a_file_from_the_version_before_is_dropped_without_fuss() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("restore");
    let token = RestoreToken::mint([4; 16]);
    std::fs::write(
        &path,
        format!("transom-restore 1\n{}\tapp\toutput:DP-1\n", token.expose()),
    )
    .expect("write");
    let store = FileStore::new(path.clone());
    assert!(store.load().expect("an empty book").is_empty());
    assert!(!store.bad_path().exists(), "nothing was damaged");
}

#[test]
fn a_fifo_or_a_huge_file_is_not_read_for_ever() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("restore");
    let made = std::process::Command::new("mkfifo").arg(&path).status();
    if !made.is_ok_and(|status| status.success()) {
        eprintln!("SKIPPED: no mkfifo");
        return;
    }
    assert!(matches!(
        FileStore::new(path).load(),
        Err(StoreError::Damaged)
    ));
}

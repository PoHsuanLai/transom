use transom::access::Question;
use transom::names::{AppNames, MAX_LABEL_CHARS, entry_name};

fn data_dir_with(entries: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let apps = dir.path().join("applications");
    std::fs::create_dir_all(&apps).expect("applications");
    for (file, text) in entries {
        std::fs::write(apps.join(file), text).expect("write");
    }
    dir
}

#[test]
fn an_app_is_shown_by_its_desktop_entry_else_by_its_whole_id() {
    let dir = data_dir_with(&[
        (
            "org.example.Meet.desktop",
            "[Desktop Entry]\nName=Meet\nName[zh_TW]=\u{6703}\u{8B70}\nName[de]=Treffen\n[Desktop Action x]\nName=Other\n",
        ),
        (
            "org.example.Bad.desktop",
            "[Desktop Entry]\nName=Bad\u{202E}gnp\u{7}\n",
        ),
    ]);
    let names = AppNames::new(
        vec![dir.path().to_path_buf()],
        vec!["zh_TW".to_owned(), "zh".to_owned()],
    );
    let plain = AppNames::new(vec![dir.path().to_path_buf()], vec![]);
    let table: [(&AppNames, &str, &str); 9] = [
        (&names, "org.example.Meet", "\u{6703}\u{8B70}"),
        (&plain, "org.example.Meet", "Meet"),
        (&plain, "org.example.Meet.desktop", "Meet"),
        (&plain, "org.example.Bad", "Badgnp"),
        // No entry: the whole id, never only its last part.
        (
            &plain,
            "org.evil.System-Settings",
            "org.evil.System-Settings",
        ),
        (&AppNames::none(), "org.example.Meet", "org.example.Meet"),
        // No id at all.
        (&plain, "", "An app"),
        (&plain, ".", "."),
        // An id that is not a plain file name is not looked up.
        (&plain, "../../etc/passwd", "../../etc/passwd"),
    ];
    for (names, id, want) in table {
        assert_eq!(names.label(id), want, "id {id:?}");
    }
}

#[test]
fn a_shown_name_has_no_hidden_characters_and_is_cut() {
    let names = AppNames::none();
    let long = format!("com.example.{}", "x".repeat(10_000));
    let shown = names.label(&long);
    assert_eq!(shown.chars().count(), MAX_LABEL_CHARS);
    assert!(shown.ends_with('\u{2026}'));
    assert_eq!(names.label("a\u{202E}b\u{200B}c\nd"), "abcd");
}

#[test]
fn an_entry_without_a_name_in_its_main_group_gives_none() {
    assert_eq!(entry_name("[Other]\nName=x\n", &[]), None);
    assert_eq!(entry_name("[Desktop Entry]\nExec=x\n", &[]), None);
}

#[test]
fn a_fifo_in_place_of_an_entry_does_not_block_and_falls_back_to_the_id() {
    let dir = data_dir_with(&[]);
    let fifo = dir
        .path()
        .join("applications")
        .join("org.example.Pipe.desktop");
    let made = std::process::Command::new("mkfifo").arg(&fifo).status();
    if !made.is_ok_and(|status| status.success()) {
        eprintln!("SKIPPED: no mkfifo");
        return;
    }
    let names = AppNames::new(vec![dir.path().to_path_buf()], vec![]);
    assert_eq!(names.label("org.example.Pipe"), "org.example.Pipe");
}

#[test]
fn the_texts_of_a_question_are_cleaned_and_cut() {
    let mut options = transom::screencast::Dict::new();
    options.insert(
        "grant_label".to_owned(),
        zbus::zvariant::OwnedValue::try_from(zbus::zvariant::Value::new("Share\u{202E}!"))
            .expect("a string"),
    );
    let question = Question::from_call(
        "Meet",
        &"t".repeat(500),
        "line\none",
        "para\none\n\u{7}two",
        &options,
    );
    assert_eq!(question.title.chars().count(), 128);
    assert_eq!(question.subtitle, "lineone");
    assert_eq!(question.body, "para\none\ntwo");
    assert_eq!(question.grant_label, "Share!");
}

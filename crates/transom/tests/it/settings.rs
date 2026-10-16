use std::sync::atomic::{AtomicU32, Ordering};

use transom::settings::{
    AppearanceError, AppearanceSource, DesktopAppearance, PortalError, SettingsService,
    parse_appearance, read_namespaces, read_setting,
};
use transom_core::{Appearance, Contrast, Scheme};

const DOCUMENT: &str = r#"
# the desktop's look
[appearance]
theme = "Dark"   # forced
accent = "green"

[other]
theme = "light"
"#;

#[test]
fn the_document_gives_a_scheme_and_an_accent() {
    let table = [
        ("", Scheme::NoPreference, false),
        ("[appearance]\ntheme = \"light\"", Scheme::Light, false),
        (
            "[appearance]\ntheme = \"system\"\naccent = \"red\"",
            Scheme::NoPreference,
            true,
        ),
        (
            "[appearance]\naccent = \"mauve\"",
            Scheme::NoPreference,
            false,
        ),
        ("theme = \"dark\"", Scheme::NoPreference, false),
        (DOCUMENT, Scheme::Dark, true),
    ];
    for (text, scheme, has_accent) in table {
        let got = parse_appearance(text);
        assert_eq!(got.scheme, scheme, "{text}");
        assert_eq!(got.accent.is_some(), has_accent, "{text}");
        assert_eq!(got.contrast, Contrast::NoPreference);
    }
}

#[test]
fn a_missing_document_means_nothing_is_chosen() {
    let dir = tempfile::tempdir().expect("tempdir");
    let source = DesktopAppearance::at(dir.path().join("appearance.toml"));
    let got = source.current().expect("a missing file is not an error");
    assert_eq!(got.scheme, Scheme::NoPreference);
    assert!(got.accent.is_none());
}

#[test]
fn a_written_document_is_read_again_each_time() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("appearance.toml");
    let source = DesktopAppearance::at(path.clone());
    std::fs::write(&path, "[appearance]\ntheme = \"dark\"\n").expect("write");
    assert_eq!(source.current().expect("read").scheme, Scheme::Dark);
    std::fs::write(&path, "[appearance]\ntheme = \"light\"\n").expect("write");
    assert_eq!(source.current().expect("read").scheme, Scheme::Light);
}

fn dark_green() -> Appearance {
    parse_appearance(DOCUMENT)
}

#[test]
fn reads_convert_to_the_wire_shapes() {
    let a = dark_green();
    let ns = "org.freedesktop.appearance";
    let scheme = read_setting(&a, ns, "color-scheme").expect("scheme");
    assert_eq!(u32::try_from(scheme).expect("a u32"), 1);
    let accent = read_setting(&a, ns, "accent-color").expect("accent");
    assert_eq!(accent.value_signature().to_string(), "(ddd)");
    let contrast = read_setting(&a, ns, "contrast").expect("contrast");
    assert_eq!(u32::try_from(contrast).expect("a u32"), 0);
}

#[test]
fn an_unknown_setting_is_not_found() {
    let a = dark_green();
    for (ns, key) in [("org.example", "x"), ("org.freedesktop.appearance", "nope")] {
        assert!(
            matches!(read_setting(&a, ns, key), Err(PortalError::NotFound(_))),
            "{ns} {key}"
        );
    }
}

#[test]
fn read_all_groups_by_namespace_and_honours_patterns() {
    let a = dark_green();
    let table: [(&[&str], usize); 4] = [
        (&[], 3),
        (&["org.freedesktop.appearance"], 3),
        (&["org.freedesktop.*"], 3),
        (&["org.gnome.*"], 0),
    ];
    for (patterns, keys) in table {
        let patterns: Vec<String> = patterns.iter().map(|p| (*p).to_owned()).collect();
        let got = read_namespaces(&a, &patterns);
        let count: usize = got.values().map(|m| m.len()).sum();
        assert_eq!(count, keys, "{patterns:?}");
    }
}

#[test]
fn a_document_that_is_not_a_small_regular_file_is_an_error_not_a_hang() {
    let dir = tempfile::tempdir().expect("tempdir");
    let huge = dir.path().join("huge.toml");
    std::fs::write(&huge, vec![b'#'; 65 * 1024]).expect("write");
    assert!(matches!(
        DesktopAppearance::at(huge).current(),
        Err(AppearanceError::Unreadable(_))
    ));
    let fifo = dir.path().join("fifo.toml");
    let made = std::process::Command::new("mkfifo").arg(&fifo).status();
    if made.is_ok_and(|status| status.success()) {
        assert!(matches!(
            DesktopAppearance::at(fifo).current(),
            Err(AppearanceError::Unreadable(_))
        ));
    } else {
        eprintln!("SKIPPED (fifo): no mkfifo");
    }
}

/// A source that reads once and then fails.
#[derive(Debug, Default)]
struct ReadsOnce(AtomicU32);

impl AppearanceSource for ReadsOnce {
    fn current(&self) -> Result<Appearance, AppearanceError> {
        if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
            Ok(parse_appearance("[appearance]\ntheme = \"dark\"\n"))
        } else {
            Err(AppearanceError::Unreadable("gone".to_owned()))
        }
    }
}

#[tokio::test]
async fn the_last_good_appearance_stands_when_the_document_cannot_be_read() {
    let service = SettingsService::new(Box::new(ReadsOnce::default()));
    assert_eq!(
        service.snapshot().await.expect("first").scheme,
        Scheme::Dark
    );
    assert_eq!(
        service
            .snapshot()
            .await
            .expect("the last one stands")
            .scheme,
        Scheme::Dark
    );
    assert!(service.refresh().await.is_err(), "and the watcher is told");

    let never = SettingsService::new(Box::new(DesktopAppearance::at(std::path::PathBuf::from(
        "/nonexistent-dir/x/appearance.toml",
    ))));
    assert!(
        never.snapshot().await.is_ok(),
        "a missing document means nothing is chosen"
    );
}

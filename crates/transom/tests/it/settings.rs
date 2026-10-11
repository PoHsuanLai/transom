use transom::settings::{
    AppearanceSource, DesktopAppearance, PortalError, parse_appearance, read_namespaces,
    read_setting,
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
    let mut source = DesktopAppearance::at(dir.path().join("appearance.toml"));
    let got = source.current().expect("a missing file is not an error");
    assert_eq!(got.scheme, Scheme::NoPreference);
    assert!(got.accent.is_none());
}

#[test]
fn a_written_document_is_read_again_each_time() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("appearance.toml");
    let mut source = DesktopAppearance::at(path.clone());
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

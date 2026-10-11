use transom_core::settings_values::APPEARANCE;
use transom_core::{
    Appearance, Contrast, Rgb, Scheme, SettingValue, SettingsRefusal, changes, read, read_all,
};

fn dark() -> Appearance {
    Appearance {
        scheme: Scheme::Dark,
        accent: Some(Rgb::from_u8(255, 0, 0)),
        contrast: Contrast::NoPreference,
    }
}

#[test]
fn reads_map_to_the_specifications_numbers() {
    let light = Appearance {
        scheme: Scheme::Light,
        accent: None,
        contrast: Contrast::Higher,
    };
    let none = Appearance {
        scheme: Scheme::NoPreference,
        ..light
    };
    let table = [
        (dark(), "color-scheme", SettingValue::U32(1)),
        (light, "color-scheme", SettingValue::U32(2)),
        (none, "color-scheme", SettingValue::U32(0)),
        (dark(), "contrast", SettingValue::U32(0)),
        (light, "contrast", SettingValue::U32(1)),
        (
            dark(),
            "accent-color",
            SettingValue::Rgb(Rgb {
                r: 1.0,
                g: 0.0,
                b: 0.0,
            }),
        ),
    ];
    for (appearance, key, want) in table {
        assert_eq!(read(&appearance, APPEARANCE, key), Ok(want), "{key}");
    }
}

#[test]
fn no_accent_is_out_of_range() {
    let a = Appearance {
        accent: None,
        ..dark()
    };
    match read(&a, APPEARANCE, "accent-color") {
        Ok(SettingValue::Rgb(c)) => assert!(c.r < 0.0 && c.g < 0.0 && c.b < 0.0),
        other => panic!("{other:?}"),
    }
}

#[test]
fn unknown_keys_and_namespaces_are_not_found() {
    assert_eq!(
        read(&dark(), APPEARANCE, "nope"),
        Err(SettingsRefusal::NotFound)
    );
    assert_eq!(
        read(&dark(), "org.example", "color-scheme"),
        Err(SettingsRefusal::NotFound)
    );
}

#[test]
fn namespace_patterns() {
    let a = dark();
    let none: [&str; 0] = [];
    let table: [(&[&str], usize); 7] = [
        (&none, 3),
        (&[""], 3),
        (&["org.freedesktop.appearance"], 3),
        (&["org.freedesktop.*"], 3),
        (&["org.*", "x"], 3),
        (&["org.gnome.*"], 0),
        (&["org.freedesktop.appearanc"], 0),
    ];
    for (patterns, want) in table {
        assert_eq!(read_all(&a, patterns).len(), want, "{patterns:?}");
    }
}

#[test]
fn only_what_differs_is_a_change() {
    let a = dark();
    assert!(changes(&a, &a).is_empty());
    let b = Appearance {
        scheme: Scheme::Light,
        ..a
    };
    let got = changes(&a, &b);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].0.key, "color-scheme");
    assert_eq!(got[0].1, SettingValue::U32(2));
    let c = Appearance {
        accent: None,
        contrast: Contrast::Higher,
        ..b
    };
    assert_eq!(changes(&b, &c).len(), 2);
}

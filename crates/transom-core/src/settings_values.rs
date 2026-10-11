//! The values the Settings portal publishes: the `org.freedesktop.appearance` namespace.
//!
//! The portal's contract is fixed by the freedesktop specification, so this module is that
//! contract as data: given the appearance the desktop currently has, which keys exist, what they
//! hold, which a namespace pattern selects and which changed. Where the appearance comes from
//! (the desktop's settings document) is the caller's seam.

/// The one namespace this backend serves.
pub const APPEARANCE: &str = "org.freedesktop.appearance";

/// A colour, each channel in 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb {
    /// Red.
    pub r: f64,
    /// Green.
    pub g: f64,
    /// Blue.
    pub b: f64,
}

impl Rgb {
    /// A colour from 8-bit channels.
    pub fn from_u8(r: u8, g: u8, b: u8) -> Self {
        Self {
            r: f64::from(r) / 255.0,
            g: f64::from(g) / 255.0,
            b: f64::from(b) / 255.0,
        }
    }
}

/// Light or dark.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Scheme {
    /// The person has no preference.
    NoPreference,
    /// Dark.
    Dark,
    /// Light.
    Light,
}

/// Contrast.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Contrast {
    /// The usual contrast.
    NoPreference,
    /// Higher contrast.
    Higher,
}

/// What the desktop's appearance is, as far as apps may ask.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Appearance {
    /// Light or dark.
    pub scheme: Scheme,
    /// The accent colour, if the person chose one.
    pub accent: Option<Rgb>,
    /// Contrast.
    pub contrast: Contrast,
}

/// A namespace and a key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SettingKey {
    /// The namespace.
    pub namespace: &'static str,
    /// The key in it.
    pub key: &'static str,
}

/// A value, in the shapes the portal uses.
#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum SettingValue {
    /// A `u`.
    U32(u32),
    /// A `(ddd)`.
    Rgb(Rgb),
}

/// Why a read failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SettingsRefusal {
    /// No such namespace or key.
    #[error("no such setting")]
    NotFound,
}

const COLOR_SCHEME: SettingKey = SettingKey {
    namespace: APPEARANCE,
    key: "color-scheme",
};
const ACCENT_COLOR: SettingKey = SettingKey {
    namespace: APPEARANCE,
    key: "accent-color",
};
const CONTRAST: SettingKey = SettingKey {
    namespace: APPEARANCE,
    key: "contrast",
};

/// The specification's "no accent chosen": a colour outside 0 to 1.
const NO_ACCENT: Rgb = Rgb {
    r: -1.0,
    g: -1.0,
    b: -1.0,
};

fn value_of(appearance: &Appearance, key: SettingKey) -> Option<SettingValue> {
    if key == COLOR_SCHEME {
        let n = match appearance.scheme {
            Scheme::NoPreference => 0,
            Scheme::Dark => 1,
            Scheme::Light => 2,
        };
        Some(SettingValue::U32(n))
    } else if key == ACCENT_COLOR {
        Some(SettingValue::Rgb(appearance.accent.unwrap_or(NO_ACCENT)))
    } else if key == CONTRAST {
        let n = match appearance.contrast {
            Contrast::NoPreference => 0,
            Contrast::Higher => 1,
        };
        Some(SettingValue::U32(n))
    } else {
        None
    }
}

const ALL_KEYS: [SettingKey; 3] = [COLOR_SCHEME, ACCENT_COLOR, CONTRAST];

/// Whether a namespace pattern from `ReadAll` selects `namespace`: an empty pattern selects
/// everything, a trailing `*` is a prefix, anything else must match exactly.
fn selects(pattern: &str, namespace: &str) -> bool {
    if pattern.is_empty() {
        return true;
    }
    match pattern.strip_suffix('*') {
        Some(prefix) => namespace.starts_with(prefix),
        None => pattern == namespace,
    }
}

/// Read one setting.
pub fn read(
    appearance: &Appearance,
    namespace: &str,
    key: &str,
) -> Result<SettingValue, SettingsRefusal> {
    ALL_KEYS
        .iter()
        .find(|k| k.namespace == namespace && k.key == key)
        .and_then(|k| value_of(appearance, *k))
        .ok_or(SettingsRefusal::NotFound)
}

/// Every setting a list of namespace patterns selects; no patterns selects everything.
pub fn read_all<S: AsRef<str>>(
    appearance: &Appearance,
    patterns: &[S],
) -> Vec<(SettingKey, SettingValue)> {
    ALL_KEYS
        .iter()
        .filter(|k| {
            patterns.is_empty() || patterns.iter().any(|p| selects(p.as_ref(), k.namespace))
        })
        .filter_map(|k| value_of(appearance, *k).map(|v| (*k, v)))
        .collect()
}

/// The settings whose value differs between two appearances (for `SettingChanged`).
pub fn changes(old: &Appearance, new: &Appearance) -> Vec<(SettingKey, SettingValue)> {
    ALL_KEYS
        .iter()
        .filter_map(|k| {
            let after = value_of(new, *k)?;
            (value_of(old, *k) != Some(after)).then_some((*k, after))
        })
        .collect()
}

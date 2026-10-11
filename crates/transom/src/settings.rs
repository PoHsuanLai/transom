//! `org.freedesktop.impl.portal.Settings`: the appearance apps ask for (light or dark, accent,
//! contrast). The values and their wire numbers are `transom_core::settings_values`; the
//! appearance itself comes from the desktop's settings through [`AppearanceSource`].

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

use transom_core::{
    Appearance, Contrast, Rgb, Scheme, SettingKey, SettingValue, SettingsRefusal, changes,
};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedValue, StructureBuilder, Value};

use crate::{PORTAL_PATH, TransomError};

/// How often the watcher looks at the appearance. The document is small and changes by hand, so a
/// slow look is enough.
pub const WATCH_EVERY: Duration = Duration::from_secs(2);

/// Why the appearance could not be read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AppearanceError {
    /// The desktop's settings could not be read.
    #[error("the desktop settings could not be read: {0}")]
    Unreadable(String),
}

/// The seam: where the current appearance comes from.
pub trait AppearanceSource: Send + std::fmt::Debug {
    /// The appearance right now.
    fn current(&mut self) -> Result<Appearance, AppearanceError>;
}

/// The appearance when nothing is chosen: no scheme preference, no accent, normal contrast.
const UNCHOSEN: Appearance = Appearance {
    scheme: Scheme::NoPreference,
    accent: None,
    contrast: Contrast::NoPreference,
};

/// The real source: the desktop's `appearance.toml`, read from the path it was given. Without a
/// path, or while the file does not exist, nothing is chosen.
#[derive(Debug, Default)]
pub struct DesktopAppearance {
    path: Option<PathBuf>,
}

impl DesktopAppearance {
    /// A source with no document: nothing is chosen.
    pub fn new() -> Self {
        Self::default()
    }

    /// A source that reads the document at `path` each time it is asked.
    pub fn at(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }
}

impl AppearanceSource for DesktopAppearance {
    fn current(&mut self) -> Result<Appearance, AppearanceError> {
        let Some(path) = &self.path else {
            return Ok(UNCHOSEN);
        };
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(parse_appearance(&text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(UNCHOSEN),
            Err(e) => Err(AppearanceError::Unreadable(e.to_string())),
        }
    }
}

/// The accents the desktop offers, by the word the document stores, as sRGB.
const ACCENTS: [(&str, [u8; 3]); 8] = [
    ("blue", [0, 122, 255]),
    ("purple", [175, 82, 222]),
    ("pink", [255, 45, 85]),
    ("red", [255, 59, 48]),
    ("orange", [255, 149, 0]),
    ("yellow", [255, 204, 0]),
    ("green", [40, 205, 65]),
    ("graphite", [142, 142, 147]),
];

/// The appearance a document's `[appearance]` table holds: `theme` (`light`, `dark`, anything
/// else follows the desktop's default) and `accent` (one of the eight accent words). Every other
/// line, table and bad value is ignored, so a half-written file never fails a read.
pub fn parse_appearance(text: &str) -> Appearance {
    let mut found = UNCHOSEN;
    let mut in_table = false;
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_table = name.trim() == "appearance";
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !in_table {
            continue;
        }
        let value = value.trim().trim_matches('"').to_ascii_lowercase();
        match key.trim() {
            "theme" => {
                found.scheme = match value.as_str() {
                    "light" => Scheme::Light,
                    "dark" => Scheme::Dark,
                    _ => Scheme::NoPreference,
                };
            }
            "accent" => {
                found.accent = ACCENTS
                    .iter()
                    .find(|(word, _)| *word == value)
                    .map(|(_, [r, g, b])| Rgb::from_u8(*r, *g, *b));
            }
            _ => {}
        }
    }
    found
}

/// A source that serves a fixed appearance, for tests.
#[derive(Debug)]
pub struct FixedAppearance(pub Appearance);

impl AppearanceSource for FixedAppearance {
    fn current(&mut self) -> Result<Appearance, AppearanceError> {
        Ok(self.0)
    }
}

/// `a{sa{sv}}`: namespace to key to value.
pub type Namespaces = HashMap<String, HashMap<String, OwnedValue>>;

/// What the portal answers with when a read fails.
#[derive(Debug, zbus::DBusError)]
#[zbus(prefix = "org.freedesktop.portal.Error")]
#[non_exhaustive]
pub enum PortalError {
    /// The bus itself failed.
    #[zbus(error)]
    ZBus(zbus::Error),
    /// No such namespace or key (`org.freedesktop.portal.Error.NotFound`).
    NotFound(String),
    /// The appearance could not be read (`org.freedesktop.portal.Error.Failed`).
    Failed(String),
}

/// A setting as the bus carries it: `U32` is `u`, `Rgb` is `(ddd)`.
pub fn wire_value(value: SettingValue) -> Option<Value<'static>> {
    match value {
        SettingValue::U32(n) => Some(Value::U32(n)),
        SettingValue::Rgb(Rgb { r, g, b }) => StructureBuilder::new()
            .add_field(r)
            .add_field(g)
            .add_field(b)
            .build()
            .ok()
            .map(Value::Structure),
        _ => None,
    }
}

fn owned(value: SettingValue) -> Option<OwnedValue> {
    wire_value(value).and_then(|v| OwnedValue::try_from(v).ok())
}

/// One setting of `appearance`, ready for the reply; an unknown one is `NotFound`.
pub fn read_setting(
    appearance: &Appearance,
    namespace: &str,
    key: &str,
) -> Result<OwnedValue, PortalError> {
    let value = transom_core::read(appearance, namespace, key).map_err(|e| match e {
        SettingsRefusal::NotFound => PortalError::NotFound(format!("{namespace} {key}")),
        _ => PortalError::Failed(e.to_string()),
    })?;
    owned(value).ok_or_else(|| PortalError::Failed("the value has no wire form".into()))
}

/// Every setting the namespace patterns select, grouped by namespace.
pub fn read_namespaces(appearance: &Appearance, patterns: &[String]) -> Namespaces {
    let mut out = Namespaces::new();
    for (key, value) in transom_core::read_all(appearance, patterns) {
        if let Some(v) = owned(value) {
            out.entry(key.namespace.to_owned())
                .or_default()
                .insert(key.key.to_owned(), v);
        }
    }
    out
}

#[derive(Debug)]
struct Tracked {
    source: Box<dyn AppearanceSource>,
    last: Option<Appearance>,
}

/// The served interface.
#[derive(Debug)]
pub struct SettingsService {
    tracked: Mutex<Tracked>,
}

impl SettingsService {
    /// A service reading from `appearance`.
    pub fn new(appearance: Box<dyn AppearanceSource>) -> Self {
        Self {
            tracked: Mutex::new(Tracked {
                source: appearance,
                last: None,
            }),
        }
    }

    /// The appearance now, remembered as what apps last saw.
    fn snapshot(&self) -> Result<Appearance, AppearanceError> {
        let mut t = self.tracked.lock().unwrap_or_else(|p| p.into_inner());
        let now = t.source.current()?;
        t.last = Some(now);
        Ok(now)
    }

    /// Look at the appearance again; the settings that differ from what apps last saw. The first
    /// look only sets the baseline.
    pub fn refresh(&self) -> Result<Vec<(SettingKey, SettingValue)>, AppearanceError> {
        let mut t = self.tracked.lock().unwrap_or_else(|p| p.into_inner());
        let now = t.source.current()?;
        let diff = t.last.map(|old| changes(&old, &now)).unwrap_or_default();
        t.last = Some(now);
        Ok(diff)
    }
}

/// Watch the appearance on the connection and emit `SettingChanged` for each difference, until
/// the connection goes away. Spawn it once the service is served.
pub fn spawn_watch(connection: zbus::Connection) {
    tokio::spawn(async move {
        if let Err(error) = interface::watch(connection, WATCH_EVERY).await {
            tracing::warn!(%error, "the appearance watcher stopped");
        }
    });
}

// The D-Bus interface lives in a private module: zbus's macro generates undocumented
// property-change methods, which the crate's `missing_docs` would flag on a public impl.
mod interface {
    use super::*;

    /// The watcher loop behind [`spawn_watch`].
    pub(super) async fn watch(
        connection: zbus::Connection,
        every: Duration,
    ) -> Result<(), TransomError> {
        let iface = connection
            .object_server()
            .interface::<_, SettingsService>(PORTAL_PATH)
            .await?;
        loop {
            tokio::time::sleep(every).await;
            let diff = match iface.get().await.refresh() {
                Ok(diff) => diff,
                Err(error) => {
                    tracing::warn!(%error, "the appearance could not be read");
                    continue;
                }
            };
            for (key, value) in diff {
                let Some(wire) = wire_value(value) else {
                    continue;
                };
                SettingsService::setting_changed(
                    iface.signal_emitter(),
                    key.namespace,
                    key.key,
                    wire,
                )
                .await?;
            }
        }
    }

    #[zbus::interface(name = "org.freedesktop.impl.portal.Settings")]
    impl SettingsService {
        /// Every setting in the namespaces the patterns select (none: all).
        async fn read_all(&self, namespaces: Vec<String>) -> Result<Namespaces, PortalError> {
            let appearance = self
                .snapshot()
                .map_err(|e| PortalError::Failed(e.to_string()))?;
            Ok(read_namespaces(&appearance, &namespaces))
        }

        /// One setting; an unknown one is a `org.freedesktop.portal.Error.NotFound`.
        async fn read(&self, namespace: &str, key: &str) -> Result<OwnedValue, PortalError> {
            let appearance = self
                .snapshot()
                .map_err(|e| PortalError::Failed(e.to_string()))?;
            read_setting(&appearance, namespace, key)
        }

        /// A setting changed.
        #[zbus(signal)]
        async fn setting_changed(
            emitter: &SignalEmitter<'_>,
            namespace: &str,
            key: &str,
            value: zbus::zvariant::Value<'_>,
        ) -> zbus::Result<()>;

        /// The interface version.
        #[zbus(property, name = "version")]
        fn version(&self) -> u32 {
            2
        }
    }
}

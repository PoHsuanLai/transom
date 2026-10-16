//! `org.freedesktop.impl.portal.Settings`: the appearance apps ask for (light or dark, accent,
//! contrast). The values and their wire numbers are `transom_core::settings_values`; the
//! appearance itself comes from the desktop's settings through [`AppearanceSource`].

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use transom_core::{
    Appearance, Contrast, Rgb, Scheme, SettingKey, SettingValue, SettingsRefusal, changes,
};
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{OwnedValue, StructureBuilder, Value};

use crate::fsread::read_text;
use crate::peers::FrontendOnly;
use crate::{PORTAL_PATH, TransomError};

/// The largest appearance document that is read. The real one is a few hundred bytes.
const MAX_DOCUMENT_BYTES: u64 = 64 * 1024;

/// How long a read of the document may take before the last good appearance is used instead. A
/// disk that hangs (a network home) must not hang the portal.
const READ_DEADLINE: Duration = Duration::from_secs(2);

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

/// The seam: where the current appearance comes from. A read may block on a disk, so callers run
/// it on a blocking thread.
pub trait AppearanceSource: Send + Sync + std::fmt::Debug {
    /// The appearance right now.
    fn current(&self) -> Result<Appearance, AppearanceError>;
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
    /// The document, if it is a regular file of a sensible size; a FIFO, a device or a huge file
    /// is an error, never a hang or an allocation.
    fn current(&self) -> Result<Appearance, AppearanceError> {
        let Some(path) = &self.path else {
            return Ok(UNCHOSEN);
        };
        match read_text(path, MAX_DOCUMENT_BYTES) {
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
    fn current(&self) -> Result<Appearance, AppearanceError> {
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
    /// The caller is not the portal frontend (`org.freedesktop.portal.Error.NotAllowed`).
    NotAllowed(String),
}

/// The reply for a caller the guard refused.
fn refused(error: zbus::fdo::Error) -> PortalError {
    PortalError::NotAllowed(error.to_string())
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

/// The served interface.
#[derive(Debug)]
pub struct SettingsService {
    source: Arc<dyn AppearanceSource>,
    /// What apps last saw. Held only to read or replace it, never across a read of the document.
    last: Mutex<Option<Appearance>>,
    /// Held by a read of the document until it really ends, so a read that outlasts its deadline
    /// is not followed by another on a second thread.
    reading: Arc<tokio::sync::Mutex<()>>,
    /// Lets only the portal frontend call.
    guard: FrontendOnly,
}

impl SettingsService {
    /// A service reading from `appearance`.
    pub fn new(appearance: Box<dyn AppearanceSource>) -> Self {
        Self {
            source: Arc::from(appearance),
            last: Mutex::new(None),
            reading: Arc::default(),
            guard: FrontendOnly::default(),
        }
    }

    /// This service letting through the callers `guard` admits. Without it, no wire call passes.
    pub fn with_guard(mut self, guard: FrontendOnly) -> Self {
        self.guard = guard;
        self
    }

    /// Read the document on a blocking thread, for at most [`READ_DEADLINE`].
    async fn read_now(&self) -> Result<Appearance, AppearanceError> {
        let unreadable = |why: &str| AppearanceError::Unreadable(why.to_owned());
        let Ok(turn) = Arc::clone(&self.reading).try_lock_owned() else {
            return Err(unreadable("the last read has not finished"));
        };
        let source = Arc::clone(&self.source);
        let read = tokio::task::spawn_blocking(move || {
            let _turn = turn;
            source.current()
        });
        match tokio::time::timeout(READ_DEADLINE, read).await {
            Ok(Ok(result)) => result,
            Ok(Err(error)) => Err(unreadable(&error.to_string())),
            Err(_) => Err(unreadable("the read took too long")),
        }
    }

    fn last(&self) -> std::sync::MutexGuard<'_, Option<Appearance>> {
        self.last.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The appearance now, remembered as what apps last saw. When the document cannot be read the
    /// last good appearance stands; with none yet, the error.
    pub async fn snapshot(&self) -> Result<Appearance, AppearanceError> {
        match self.read_now().await {
            Ok(now) => {
                *self.last() = Some(now);
                Ok(now)
            }
            Err(error) => match *self.last() {
                Some(old) => {
                    tracing::warn!(%error, "the appearance could not be read; the last one stands");
                    Ok(old)
                }
                None => Err(error),
            },
        }
    }

    /// Look at the appearance again; the settings that differ from what apps last saw. The first
    /// look only sets the baseline. An unreadable document changes nothing.
    pub async fn refresh(&self) -> Result<Vec<(SettingKey, SettingValue)>, AppearanceError> {
        let now = self.read_now().await?;
        let mut last = self.last();
        let diff = last.map(|old| changes(&old, &now)).unwrap_or_default();
        *last = Some(now);
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
            let diff = match iface.get().await.refresh().await {
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
        async fn read_all(
            &self,
            namespaces: Vec<String>,
            #[zbus(header)] header: zbus::message::Header<'_>,
        ) -> Result<Namespaces, PortalError> {
            self.guard.admit(&header).await.map_err(refused)?;
            let appearance = self
                .snapshot()
                .await
                .map_err(|e| PortalError::Failed(e.to_string()))?;
            Ok(read_namespaces(&appearance, &namespaces))
        }

        /// One setting; an unknown one is a `org.freedesktop.portal.Error.NotFound`.
        async fn read(
            &self,
            namespace: &str,
            key: &str,
            #[zbus(header)] header: zbus::message::Header<'_>,
        ) -> Result<OwnedValue, PortalError> {
            self.guard.admit(&header).await.map_err(refused)?;
            let appearance = self
                .snapshot()
                .await
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

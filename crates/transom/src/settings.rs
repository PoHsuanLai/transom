//! `org.freedesktop.impl.portal.Settings`: the appearance apps ask for (light or dark, accent,
//! contrast). The values and their wire numbers are `transom_core::settings_values`; the
//! appearance itself comes from the desktop's settings through [`AppearanceSource`].

use std::collections::HashMap;
use std::sync::Mutex;

use transom_core::Appearance;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedValue;

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

/// The real source: the desktop's settings document.
#[derive(Debug, Default)]
pub struct DesktopAppearance;

impl DesktopAppearance {
    /// A source that reads on demand.
    pub fn new() -> Self {
        Self
    }
}

impl AppearanceSource for DesktopAppearance {
    fn current(&mut self) -> Result<Appearance, AppearanceError> {
        todo!(
            "P2-T1: read the appearance keys of the desktop settings document (the one home); watch it for changes"
        )
    }
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

/// The served interface.
#[derive(Debug)]
pub struct SettingsService {
    #[expect(dead_code, reason = "P2-T1")]
    appearance: Mutex<Box<dyn AppearanceSource>>,
}

impl SettingsService {
    /// A service reading from `appearance`.
    pub fn new(appearance: Box<dyn AppearanceSource>) -> Self {
        Self {
            appearance: Mutex::new(appearance),
        }
    }
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Settings")]
impl SettingsService {
    /// Every setting in the namespaces the patterns select (none: all).
    async fn read_all(&self, namespaces: Vec<String>) -> Namespaces {
        let _ = namespaces;
        todo!(
            "P2-T1: transom_core::read_all, group by namespace, wrap each SettingValue in a Value"
        )
    }

    /// One setting; an unknown one is a `org.freedesktop.portal.Error.NotFound`.
    async fn read(&self, namespace: &str, key: &str) -> zbus::fdo::Result<OwnedValue> {
        let _ = (namespace, key);
        todo!("P2-T1: transom_core::read; NotFound maps to the portal's error name")
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

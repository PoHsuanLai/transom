//! The quire desktop's portal backend: the D-Bus services `xdg-desktop-portal` forwards to.
//!
//! The pure decisions live in `transom-core`; this crate holds the I/O around them, each behind
//! a seam so tests run on a private bus with fakes:
//!
//! - [`capture`]: where pixels come from ([`capture::CaptureSource`]).
//! - [`screencast::stream`]: where pixels go ([`screencast::stream::FrameSink`]).
//! - [`screencast::restore`]: where restore tokens are kept ([`screencast::restore::RestoreStore`]).
//! - [`chooser`]: how the person is asked what to share ([`chooser::Chooser`]).
//! - [`settings::AppearanceSource`]: where the appearance comes from.
//!
//! [`serve`] puts the five interfaces on the session bus.

pub mod capture;
pub mod chooser;
pub mod error;
pub mod inhibit;
pub mod remote_desktop;
pub mod screencast;
pub mod screenshot;
pub mod settings;

use std::future::Future;
use std::pin::Pin;

pub use error::TransomError;

/// A boxed future, the shape the seams return so they stay object-safe.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The well-known name the portal frontend activates.
pub const BUS_NAME: &str = "org.freedesktop.impl.portal.desktop.quire";

/// The object path every portal backend interface is served at.
pub const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";

/// The seams a running portal is built from.
#[derive(Debug)]
#[non_exhaustive]
pub struct Parts {
    /// Asks the person what to share.
    pub chooser: Box<dyn chooser::Chooser>,
    /// Remembers restore tokens.
    pub restore: Box<dyn screencast::restore::RestoreStore>,
    /// Reads the appearance.
    pub appearance: Box<dyn settings::AppearanceSource>,
}

impl Parts {
    /// The real seams: the shell's chooser, a restore file and the desktop's settings.
    pub fn real(
        chooser: chooser::ShellChooser,
        restore: screencast::restore::FileStore,
        appearance: settings::DesktopAppearance,
    ) -> Self {
        Self {
            chooser: Box::new(chooser),
            restore: Box::new(restore),
            appearance: Box::new(appearance),
        }
    }
}

/// Claim [`BUS_NAME`] on the session bus and serve the five interfaces.
///
/// The returned connection keeps the service alive; drop it to stop.
pub async fn serve(parts: Parts) -> Result<zbus::Connection, TransomError> {
    let Parts {
        chooser,
        restore,
        appearance,
    } = parts;
    let connection = zbus::connection::Builder::session()?
        .name(BUS_NAME)?
        .serve_at(PORTAL_PATH, screenshot::ScreenshotService::new())?
        .serve_at(
            PORTAL_PATH,
            screencast::ScreenCastService::new(chooser, restore),
        )?
        .serve_at(PORTAL_PATH, remote_desktop::RemoteDesktopService::new())?
        .serve_at(PORTAL_PATH, settings::SettingsService::new(appearance))?
        .serve_at(PORTAL_PATH, inhibit::InhibitService::new())?
        .build()
        .await?;
    Ok(connection)
}

//! The quire desktop's portal backend: the D-Bus services `xdg-desktop-portal` forwards to.
//!
//! The pure decisions live in `transom-core`; this crate holds the I/O around them, each behind
//! a seam so tests run on a private bus with fakes:
//!
//! - [`capture`]: where pixels come from ([`capture::CaptureSource`]).
//! - [`screencast::stream`]: where pixels go ([`screencast::stream::FrameSink`]).
//! - [`screencast::restore`]: where restore tokens are kept ([`screencast::restore::RestoreStore`]).
//! - [`chooser`]: how the person is asked what to share ([`chooser::Chooser`]).
//! - [`remote_desktop`]: how the person is asked to allow remote control ([`remote_desktop::Consent`])
//!   and where an input socket comes from ([`remote_desktop::EisHandout`]).
//! - [`access`]: how the person is asked a yes-or-no question about an app ([`access::Asker`]).
//! - [`settings::AppearanceSource`]: where the appearance comes from.
//!
//! [`serve`] puts the six interfaces on the session bus. Every question the portal asks goes out
//! on the connection that owns its name ([`link::BusLink`]), because the shell answers only that one.

pub mod access;
pub mod capture;
pub mod chooser;
pub mod error;
pub mod inhibit;
pub mod link;
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
    /// Asks the person whether an app may control the computer.
    pub consent: Box<dyn remote_desktop::Consent>,
    /// Gets an input socket from the compositor for a remote-desktop session.
    pub eis: Box<dyn remote_desktop::EisHandout>,
    /// Asks the person yes-or-no questions about an app.
    pub access: Box<dyn access::Asker>,
    /// Takes screenshots; without a setup every screenshot is answered as failed.
    pub screenshots: screenshot::ScreenshotSetup,
    /// The connection that owns the portal's name, once [`serve`] has built it. The seams that
    /// ask the shell or the compositor hold a copy.
    pub link: link::BusLink,
}

impl Parts {
    /// The real seams: the shell's chooser, a restore file and the desktop's settings. Every seam
    /// that asks the shell or the compositor asks through one [`link::BusLink`], which [`serve`]
    /// attaches to the connection that owns the portal's name.
    pub fn real(
        restore: screencast::restore::FileStore,
        appearance: settings::DesktopAppearance,
    ) -> Self {
        let link = link::BusLink::new();
        Self {
            chooser: Box::new(chooser::ShellChooser::new(link.clone())),
            restore: Box::new(restore),
            appearance: Box::new(appearance),
            consent: Box::new(remote_desktop::ShellConsent::new(Box::new(
                chooser::ShellChooser::new(link.clone()),
            ))),
            eis: Box::new(remote_desktop::CasementEis::new(link.clone())),
            access: Box::new(access::ShellAsker::new(link.clone())),
            screenshots: screenshot::ScreenshotSetup::unconfigured(),
            link,
        }
    }

    /// The link the real seams ask through, for a seam built outside [`Parts::real`] (the colour
    /// picker of the screenshot setup).
    pub fn link(&self) -> link::BusLink {
        self.link.clone()
    }

    /// These parts with screenshots set up as `screenshots` says.
    pub fn with_screenshots(mut self, screenshots: screenshot::ScreenshotSetup) -> Self {
        self.screenshots = screenshots;
        self
    }
}

/// Claim [`BUS_NAME`] on the session bus and serve the six interfaces.
///
/// The returned connection keeps the service alive; drop it to stop.
pub async fn serve(parts: Parts) -> Result<zbus::Connection, TransomError> {
    serve_on(zbus::connection::Builder::session()?, parts).await
}

/// As [`serve`], on the bus `builder` connects to (a private bus, in tests).
///
/// The interfaces are served first, the connection is attached to `parts.link`, and only then is
/// the name claimed, so no call can arrive before the seams know which connection to ask from.
pub async fn serve_on(
    builder: zbus::connection::Builder<'_>,
    parts: Parts,
) -> Result<zbus::Connection, TransomError> {
    let Parts {
        chooser,
        restore,
        appearance,
        consent,
        eis,
        access,
        screenshots,
        link,
    } = parts;
    let connection = builder
        .serve_at(
            PORTAL_PATH,
            screenshot::ScreenshotService::with_setup(screenshots),
        )?
        .serve_at(
            PORTAL_PATH,
            screencast::ScreenCastService::new(chooser, restore),
        )?
        .serve_at(
            PORTAL_PATH,
            remote_desktop::RemoteDesktopService::new(consent, eis),
        )?
        .serve_at(PORTAL_PATH, settings::SettingsService::new(appearance))?
        .serve_at(PORTAL_PATH, inhibit::InhibitService::new())?
        .serve_at(PORTAL_PATH, access::AccessService::new(access))?
        .build()
        .await?;
    link.attach(connection.clone());
    settings::spawn_watch(connection.clone());
    connection.request_name(BUS_NAME).await?;
    Ok(connection)
}

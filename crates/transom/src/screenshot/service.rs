//! The served `org.freedesktop.impl.portal.Screenshot` interface.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use transom_core::{Rgb, SettingValue};
use zbus::zvariant::{ObjectPath, OwnedValue, Value};

use super::{ScreenshotSetup, file_uri, take};
use crate::access::{Question, response_of};
use crate::chooser::ChooserError;
use crate::screencast::Dict;
use crate::settings::wire_value;

/// The portal's response codes.
const RESPONSE_OK: u32 = 0;
const RESPONSE_CANCELLED: u32 = 1;
const RESPONSE_FAILED: u32 = 2;

/// Who asks the person before a picture is taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Asks {
    /// The portal frontend asked and keeps the answer; asking again would be a second dialog.
    Frontend,
    /// Nobody has: this backend asks.
    Backend,
}

fn flag(options: &Dict, key: &str) -> Option<bool> {
    options.get(key).and_then(|v| bool::try_from(v).ok())
}

/// The frontend asks only for a request that is not interactive and says it checked its
/// permission store. An interactive request is one the frontend leaves to the backend's own
/// dialog (it skips its question for it, whatever else the options say), and a request that does
/// not say the frontend checked did not come through the frontend's question at all.
fn asks(options: &Dict) -> Asks {
    let interactive = flag(options, "interactive") == Some(true);
    let checked = flag(options, "permission_store_checked") == Some(true);
    if checked && !interactive {
        Asks::Frontend
    } else {
        Asks::Backend
    }
}

/// The question for a picture of the whole screen.
fn screenshot_question(app_label: &str) -> Question {
    Question {
        app_label: app_label.to_owned(),
        title: "Allow a screenshot?".to_owned(),
        subtitle: String::new(),
        body: format!("{app_label} wants to take a picture of everything showing on your screen."),
        grant_label: "Allow".to_owned(),
        deny_label: "Don\u{2019}t Allow".to_owned(),
    }
}

fn owned(value: impl Into<Value<'static>>) -> Option<OwnedValue> {
    OwnedValue::try_from(value.into()).ok()
}

/// A colour as the portal's `color` result carries it: `(ddd)`.
fn wire_color(color: Rgb) -> Option<OwnedValue> {
    wire_value(SettingValue::Rgb(color)).and_then(|v| OwnedValue::try_from(v).ok())
}

/// The served interface.
#[derive(Debug)]
pub struct ScreenshotService {
    setup: Arc<ScreenshotSetup>,
    seq: AtomicU32,
}

impl Default for ScreenshotService {
    fn default() -> Self {
        Self::with_setup(ScreenshotSetup::unconfigured())
    }
}

impl ScreenshotService {
    /// A service with no screens: every screenshot is answered as failed.
    pub fn new() -> Self {
        Self::default()
    }

    /// A service taking screenshots as `setup` says.
    pub fn with_setup(setup: ScreenshotSetup) -> Self {
        Self {
            setup: Arc::new(setup),
            seq: AtomicU32::new(0),
        }
    }

    /// Answer a `Screenshot` call: ask the person first unless the frontend already did (see
    /// [`asks`]), then take a picture of every screen. A no, a closed question or no shell to ask
    /// is a cancel and nothing is captured.
    pub async fn shoot(&self, app_id: &str, options: &Dict) -> (u32, Dict) {
        if asks(options) == Asks::Backend {
            let question = screenshot_question(&self.setup.names.label(app_id));
            let code = response_of(&self.setup.asker.ask(&question).await);
            if code != RESPONSE_OK {
                return (code, Dict::new());
            }
        }
        let setup = Arc::clone(&self.setup);
        let seq = self.seq.fetch_add(1, Ordering::Relaxed);
        let made = tokio::task::spawn_blocking(move || {
            let path = take(&setup, seq)?;
            file_uri(&path)
        })
        .await;
        match made {
            Ok(Ok(uri)) => {
                let mut results = Dict::new();
                if let Some(v) = owned(uri) {
                    results.insert("uri".to_owned(), v);
                }
                (RESPONSE_OK, results)
            }
            Ok(Err(error)) => {
                tracing::warn!(%error, "a screenshot failed");
                (RESPONSE_FAILED, Dict::new())
            }
            Err(error) => {
                tracing::warn!(%error, "the screenshot task did not finish");
                (RESPONSE_FAILED, Dict::new())
            }
        }
    }
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Screenshot")]
impl ScreenshotService {
    /// Take a screenshot of every screen, laid out as the screens sit; the reply's `uri` names the
    /// PNG written to the person's pictures. The person is asked first unless the frontend
    /// already asked (never for an `interactive` request, whose question is this backend's).
    /// The `modal` and `target` hints are not used: the whole screen is what `AvailableTargets`
    /// offers.
    async fn screenshot(
        &self,
        handle: ObjectPath<'_>,
        app_id: &str,
        parent_window: &str,
        options: Dict,
    ) -> (u32, Dict) {
        let _ = (handle, parent_window);
        self.shoot(app_id, &options).await
    }

    /// Let the person pick a colour from the screen; the reply's `color` is `(ddd)`. The shell
    /// shows the picker. With no shell to ask, or a cancel, the reply is cancelled.
    async fn pick_color(
        &self,
        handle: ObjectPath<'_>,
        app_id: &str,
        parent_window: &str,
        options: Dict,
    ) -> (u32, Dict) {
        let _ = (handle, parent_window, options);
        match self
            .setup
            .picker
            .pick(&self.setup.names.label(app_id))
            .await
        {
            Ok(Some(color)) => match wire_color(color) {
                Some(value) => {
                    let mut results = Dict::new();
                    results.insert("color".to_owned(), value);
                    (RESPONSE_OK, results)
                }
                None => (RESPONSE_FAILED, Dict::new()),
            },
            Ok(None) => (RESPONSE_CANCELLED, Dict::new()),
            Err(ChooserError::Unavailable) => {
                tracing::warn!("no colour picker is available");
                (RESPONSE_CANCELLED, Dict::new())
            }
            Err(error) => {
                tracing::warn!(%error, "the colour picker failed");
                (RESPONSE_FAILED, Dict::new())
            }
        }
    }

    /// The screenshot targets offered: the whole screen.
    #[zbus(property)]
    fn available_targets(&self) -> u32 {
        1
    }

    /// The interface version.
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        3
    }
}

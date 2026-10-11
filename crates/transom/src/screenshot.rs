//! `org.freedesktop.impl.portal.Screenshot`: a picture of the screen, and a colour picked from it.

use zbus::zvariant::ObjectPath;

use crate::screencast::Dict;

/// The served interface.
#[derive(Debug, Default)]
pub struct ScreenshotService;

impl ScreenshotService {
    /// A service.
    pub fn new() -> Self {
        Self::default()
    }
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Screenshot")]
impl ScreenshotService {
    /// Take a screenshot; the reply's `uri` names the PNG written to the person's pictures.
    async fn screenshot(
        &self,
        handle: ObjectPath<'_>,
        app_id: &str,
        parent_window: &str,
        options: Dict,
    ) -> (u32, Dict) {
        let _ = (handle, app_id, parent_window, options);
        todo!("P2-T1: capture every output with ExtCapture, compose, encode PNG, write, reply uri")
    }

    /// Let the person pick a colour from the screen; the reply's `color` is `(ddd)`.
    async fn pick_color(
        &self,
        handle: ObjectPath<'_>,
        app_id: &str,
        parent_window: &str,
        options: Dict,
    ) -> (u32, Dict) {
        let _ = (handle, app_id, parent_window, options);
        todo!("P2-T1: a picker through the shell, reply color")
    }

    /// The interface version.
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        2
    }
}

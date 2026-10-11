//! `org.freedesktop.impl.portal.RemoteDesktop`: an app drives the keyboard, pointer and touch.
//!
//! Input goes to the compositor over EIS (`ConnectToEIS`), where it arrives tagged as remote so
//! it can never pass for the person's own keystrokes. The compositor half is casement's
//! `casement-agent/src/eis.rs`.

use std::sync::Mutex;

use zbus::zvariant::{ObjectPath, OwnedFd};

use crate::screencast::Dict;

/// The device kinds an app may ask for: keyboard, pointer, touchscreen (1, 2, 4).
const DEVICE_TYPES: u32 = 7;

/// The served interface.
#[derive(Debug, Default)]
pub struct RemoteDesktopService {
    #[expect(dead_code, reason = "P2-T3")]
    sessions: Mutex<Vec<String>>,
}

impl RemoteDesktopService {
    /// A service with no sessions.
    pub fn new() -> Self {
        Self::default()
    }
}

#[zbus::interface(name = "org.freedesktop.impl.portal.RemoteDesktop")]
impl RemoteDesktopService {
    /// Open a session.
    async fn create_session(
        &self,
        handle: ObjectPath<'_>,
        session_handle: ObjectPath<'_>,
        app_id: &str,
        options: Dict,
    ) -> (u32, Dict) {
        let _ = (handle, session_handle, app_id, options);
        todo!("P2-T3: register the session")
    }

    /// Record which device kinds the app wants.
    async fn select_devices(
        &self,
        handle: ObjectPath<'_>,
        session_handle: ObjectPath<'_>,
        app_id: &str,
        options: Dict,
    ) -> (u32, Dict) {
        let _ = (handle, session_handle, app_id, options);
        todo!("P2-T3: store the device mask")
    }

    /// Ask the person to allow it (and what to share, if screen sharing is combined).
    async fn start(
        &self,
        handle: ObjectPath<'_>,
        session_handle: ObjectPath<'_>,
        app_id: &str,
        parent_window: &str,
        options: Dict,
    ) -> (u32, Dict) {
        let _ = (handle, session_handle, app_id, parent_window, options);
        todo!("P2-T3: ask through the shell, reply devices")
    }

    /// Relative pointer motion.
    async fn notify_pointer_motion(
        &self,
        session_handle: ObjectPath<'_>,
        options: Dict,
        dx: f64,
        dy: f64,
    ) {
        let _ = (session_handle, options, dx, dy);
        todo!("P2-T3: send to the EIS device")
    }

    /// Pointer motion to a position in a stream.
    async fn notify_pointer_motion_absolute(
        &self,
        session_handle: ObjectPath<'_>,
        options: Dict,
        stream: u32,
        x: f64,
        y: f64,
    ) {
        let _ = (session_handle, options, stream, x, y);
        todo!("P2-T3: map stream coordinates to the output")
    }

    /// A pointer button.
    async fn notify_pointer_button(
        &self,
        session_handle: ObjectPath<'_>,
        options: Dict,
        button: i32,
        state: u32,
    ) {
        let _ = (session_handle, options, button, state);
        todo!("P2-T3: send to the EIS device")
    }

    /// Smooth scrolling.
    async fn notify_pointer_axis(
        &self,
        session_handle: ObjectPath<'_>,
        options: Dict,
        dx: f64,
        dy: f64,
    ) {
        let _ = (session_handle, options, dx, dy);
        todo!("P2-T3: send to the EIS device")
    }

    /// Wheel clicks.
    async fn notify_pointer_axis_discrete(
        &self,
        session_handle: ObjectPath<'_>,
        options: Dict,
        axis: u32,
        steps: i32,
    ) {
        let _ = (session_handle, options, axis, steps);
        todo!("P2-T3: send to the EIS device")
    }

    /// A key by its evdev code.
    async fn notify_keyboard_keycode(
        &self,
        session_handle: ObjectPath<'_>,
        options: Dict,
        keycode: i32,
        state: u32,
    ) {
        let _ = (session_handle, options, keycode, state);
        todo!("P2-T3: send to the EIS device")
    }

    /// A key by its keysym.
    async fn notify_keyboard_keysym(
        &self,
        session_handle: ObjectPath<'_>,
        options: Dict,
        keysym: i32,
        state: u32,
    ) {
        let _ = (session_handle, options, keysym, state);
        todo!("P2-T3: translate through the keymap, send to the EIS device")
    }

    /// A touch begins.
    async fn notify_touch_down(
        &self,
        session_handle: ObjectPath<'_>,
        options: Dict,
        stream: u32,
        slot: u32,
        x: f64,
        y: f64,
    ) {
        let _ = (session_handle, options, stream, slot, x, y);
        todo!("P2-T3: send to the EIS device")
    }

    /// A touch moves.
    async fn notify_touch_motion(
        &self,
        session_handle: ObjectPath<'_>,
        options: Dict,
        stream: u32,
        slot: u32,
        x: f64,
        y: f64,
    ) {
        let _ = (session_handle, options, stream, slot, x, y);
        todo!("P2-T3: send to the EIS device")
    }

    /// A touch ends.
    async fn notify_touch_up(&self, session_handle: ObjectPath<'_>, options: Dict, slot: u32) {
        let _ = (session_handle, options, slot);
        todo!("P2-T3: send to the EIS device")
    }

    /// Hand the app an EIS socket for this session.
    async fn connect_to_eis(
        &self,
        session_handle: ObjectPath<'_>,
        app_id: &str,
        options: Dict,
    ) -> OwnedFd {
        let _ = (session_handle, app_id, options);
        todo!("P2-T3: ask casement for a remote-origin EIS fd")
    }

    /// The device kinds served.
    #[zbus(property)]
    fn available_device_types(&self) -> u32 {
        DEVICE_TYPES
    }

    /// The interface version.
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        2
    }
}

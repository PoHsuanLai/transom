//! Asking the person whether an app may control the computer.
//!
//! The question is its own: it says which input the app would take over, in plain words. It is
//! never the screen-sharing picker, because picking a screen to share is not saying yes to this.

use super::devices::DeviceKinds;
use crate::access::Question;

/// The question the person sees: which app, and which input it would take over.
pub fn remote_control_question(app_label: &str, kinds: DeviceKinds) -> Question {
    Question::yes_no(
        app_label,
        "Allow control of your computer?",
        format!(
            "{app_label} wants to use your {} to type and click as if it were you. \
             Only allow this for apps you trust.",
            kinds.phrase()
        ),
    )
}

//! Asking the person whether an app may control the computer.
//!
//! The question is its own: it says which input the app would take over, in plain words. It is
//! never the screen-sharing picker, because picking a screen to share is not saying yes to this.

use transom_core::{DeviceKinds, Keep, PersistMode};

use crate::access::Question;

/// The question the person sees: which app, which input it would take over, and how long a yes
/// lasts. `remembered` is what a yes will be kept for (`DoNot` for an app that is asked every
/// time), so the person never gives a lasting yes without being told.
pub fn remote_control_question(
    app_label: &str,
    kinds: DeviceKinds,
    remembered: PersistMode,
) -> Question {
    let lasts = match remembered {
        PersistMode::DoNot => String::new(),
        PersistMode::Keep(Keep::WhileRunning) => {
            format!(" {app_label} can do this again without asking until it quits.")
        }
        // Until taken back, and any longer keep a later core adds: never say less than is kept.
        _ => format!(
            " {app_label} can do this again without asking, until you take it back in Settings."
        ),
    };
    Question::yes_no(
        app_label,
        "Allow control of your computer?",
        format!(
            "{app_label} wants to use your {} to type and click as if it were you. \
             Only allow this for apps you trust.{lasts}",
            kinds.phrase()
        ),
    )
}

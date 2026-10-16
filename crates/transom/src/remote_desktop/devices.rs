//! The device kinds of a remote-control session.

/// The keyboard bit of the portal's `types`.
const KEYBOARD: u32 = 1;
/// The pointer bit.
const POINTER: u32 = 2;
/// The touchscreen bit.
const TOUCHSCREEN: u32 = 4;

/// The device kinds an app may ask for: keyboard, pointer, touchscreen (1, 2, 4).
pub(super) const DEVICE_TYPES: u32 = KEYBOARD | POINTER | TOUCHSCREEN;

/// The device kinds of a session, as the portal's `types` bit set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DeviceKinds(u32);

impl DeviceKinds {
    /// Every kind this backend offers.
    pub const ALL: Self = Self(DEVICE_TYPES);

    /// The kinds a portal value names; bits this backend does not offer are dropped.
    pub fn from_wire(value: u32) -> Self {
        Self(value & DEVICE_TYPES)
    }

    /// The portal's value.
    pub fn to_wire(self) -> u32 {
        self.0
    }

    /// Whether no kind is named.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The kinds in plain words, in a fixed order: "keyboard", "pointer", "touchscreen".
    pub fn words(self) -> Vec<&'static str> {
        [
            (KEYBOARD, "keyboard"),
            (POINTER, "pointer"),
            (TOUCHSCREEN, "touchscreen"),
        ]
        .into_iter()
        .filter(|(bit, _)| self.0 & bit != 0)
        .map(|(_, word)| word)
        .collect()
    }

    /// The kinds as one phrase: "keyboard", "keyboard and pointer", "keyboard, pointer and
    /// touchscreen".
    pub fn phrase(self) -> String {
        match self.words().as_slice() {
            [] => String::new(),
            [only] => (*only).to_owned(),
            [first @ .., last] => format!("{} and {last}", first.join(", ")),
        }
    }
}

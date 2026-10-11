//! The pure part of transom, the quire desktop's portal backend.
//!
//! Everything here is a value or a function of values, so it can be used (and tested) without a
//! bus, a PipeWire daemon or a compositor:
//!
//! - [`screencast`]: the state machine of one screen-sharing session, [`step`].
//! - [`restore`]: restore tokens and the book that remembers what they stand for.
//! - [`choice`]: what the person may pick when asked what to share, and the checks on the answer.
//! - [`inhibit`]: who asked to keep the screen on, the book of those requests.
//! - [`settings_values`]: the appearance values the Settings portal publishes.
//!
//! Randomness, time and files are arguments, never read here.

pub mod choice;
pub mod inhibit;
pub mod restore;
pub mod screencast;
pub mod settings_values;

pub use choice::{Choice, ChoiceRefusal, ChoiceRequest, Multiple, SourceKinds};
pub use inhibit::{BookChange, InhibitBook, InhibitFlags, RequestHandle};
pub use restore::{Grant, PersistMode, RestoreBook, RestoreToken};
pub use screencast::{
    CastIn, CastOut, CastReply, CastSession, CastSource, CursorMode, PwNode, step,
};
pub use settings_values::{
    Appearance, Contrast, Rgb, Scheme, SettingKey, SettingValue, SettingsRefusal, changes, read,
    read_all,
};

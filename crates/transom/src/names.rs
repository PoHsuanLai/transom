//! The app's name as the person sees it.
//!
//! The name comes from the app's own desktop entry, found by the id the portal frontend passes.
//! With no entry the full id is shown, never only its last part: `org.evil.System-Settings` must
//! not read as "System settings". Whatever is shown is stripped of control and direction
//! characters and cut to [`MAX_LABEL_CHARS`], because the id and the entry are the app's to write.

use std::path::PathBuf;
use std::sync::Arc;

use crate::app_proof::RootHeld;
use crate::fsread::read_text;

/// The most characters of an app's name that is shown or sent to the shell.
pub const MAX_LABEL_CHARS: usize = 64;

/// The largest desktop entry that is read. Real ones are a few kilobytes.
const MAX_ENTRY_BYTES: u64 = 64 * 1024;

/// The longest id that is looked up (a file name cannot be longer).
const MAX_ID_BYTES: usize = 255;

/// What an app with no id (one that is not sandboxed) is called.
const UNNAMED: &str = "An app";

/// Where desktop entries are looked for, and which languages to prefer.
#[derive(Clone, Debug, Default)]
pub struct AppNames {
    /// Data directories; an entry is `<dir>/applications/<id>.desktop`.
    dirs: Arc<[PathBuf]>,
    /// Locales in order of preference, such as `zh_TW` then `zh`.
    locales: Arc<[String]>,
}

impl AppNames {
    /// Looks nowhere: every app is shown by its id.
    pub fn none() -> Self {
        Self::default()
    }

    /// Looks in `dirs` (data directories, earlier wins), preferring `locales`.
    pub fn new(dirs: Vec<PathBuf>, locales: Vec<String>) -> Self {
        Self {
            dirs: dirs.into(),
            locales: locales.into(),
        }
    }

    /// The name to show for `app_id`: its desktop entry's `Name`, else the whole id. Never empty,
    /// never longer than [`MAX_LABEL_CHARS`], never with control or direction characters.
    ///
    /// Reads at most one small regular file per data directory; it does not block on a FIFO.
    pub fn label(&self, app_id: &str) -> String {
        let id = app_id.strip_suffix(".desktop").unwrap_or(app_id).trim();
        let named = self
            .entry_name(id)
            .map(|name| tidy(&name))
            .filter(|name| !name.is_empty());
        named.unwrap_or_else(|| {
            let shown = tidy(id);
            if shown.is_empty() {
                UNNAMED.to_owned()
            } else {
                shown
            }
        })
    }

    /// What the registered desktop entry of `app_id` says to run. The first entry found decides
    /// (earlier data directories win, as they do for the desktop), and it counts only when `root`
    /// says nobody but root can have written it: an entry in the person's home is theirs to write.
    pub fn registered(&self, app_id: &str, root: &dyn RootHeld) -> Registered {
        let id = app_id.strip_suffix(".desktop").unwrap_or(app_id).trim();
        if !plain_id(id) {
            return Registered::Nothing;
        }
        let found = self.dirs.iter().find_map(|dir| {
            let path = dir.join("applications").join(format!("{id}.desktop"));
            let text = read_text(&path, MAX_ENTRY_BYTES).ok()?;
            Some((path, text))
        });
        match found {
            None => Registered::Nothing,
            Some((path, _)) if !root.holds(&path) => Registered::NotRootHeld,
            Some((_, text)) => exec_program(&text).map_or(Registered::Nothing, Registered::Program),
        }
    }

    fn entry_name(&self, id: &str) -> Option<String> {
        if !plain_id(id) {
            return None;
        }
        self.dirs.iter().find_map(|dir| {
            let path = dir.join("applications").join(format!("{id}.desktop"));
            let text = read_text(&path, MAX_ENTRY_BYTES).ok()?;
            entry_name(&text, &self.locales)
        })
    }
}

/// What an app's registered desktop entry says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Registered {
    /// The program its `Exec` line starts, as written, from an entry only root can have written.
    Program(String),
    /// The entry that applies is in a place someone other than root can write.
    NotRootHeld,
    /// No entry, or none that names a program.
    Nothing,
}

/// The program the `Exec` line of a desktop entry's `[Desktop Entry]` group starts: its first word,
/// or the quoted first word.
pub fn exec_program(text: &str) -> Option<String> {
    let mut in_entry = false;
    for line in text.lines().map(str::trim) {
        if let Some(group) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_entry = group == "Desktop Entry";
        } else if in_entry {
            if let Some(value) = line
                .strip_prefix("Exec")
                .and_then(|rest| rest.trim_start().strip_prefix('='))
            {
                let value = value.trim();
                let word = match value.strip_prefix('"') {
                    Some(quoted) => quoted.split('"').next(),
                    None => value.split_whitespace().next(),
                };
                return word.filter(|w| !w.is_empty()).map(str::to_owned);
            }
        }
    }
    None
}

/// An id that is safe to turn into a file name: letters, digits, dot, dash, underscore.
fn plain_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_BYTES
        && !id.starts_with('.')
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

/// The `Name` of a desktop entry's `[Desktop Entry]` group in the first of `locales` that has one,
/// else the plain `Name`.
pub fn entry_name(text: &str, locales: &[String]) -> Option<String> {
    let mut in_entry = false;
    let mut plain = None;
    let mut localized: Vec<(&str, &str)> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(group) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            in_entry = group == "Desktop Entry";
            continue;
        }
        if !in_entry {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if key == "Name" {
            plain.get_or_insert(value);
        } else if let Some(locale) = key
            .strip_prefix("Name[")
            .and_then(|rest| rest.strip_suffix(']'))
        {
            localized.push((locale, value));
        }
    }
    locales
        .iter()
        .find_map(|wanted| {
            localized
                .iter()
                .find(|(locale, _)| locale == wanted)
                .map(|(_, value)| *value)
        })
        .or(plain)
        .map(str::to_owned)
}

/// Whether a character may not be shown to a person in an app's name: control characters, and the
/// characters that reorder or hide text (direction marks and overrides, zero-width characters).
fn hidden_or_reordering(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{061C}'
                | '\u{200B}'..='\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2060}'..='\u{2064}'
                | '\u{2066}'..='\u{206F}'
                | '\u{FEFF}'
        )
}

/// Whether line breaks survive [`tidy_to`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// One line: line breaks are removed with the other control characters.
    OneLine,
    /// Paragraphs: line breaks stay.
    Paragraphs,
}

/// `text` without hidden or reordering characters, trimmed, and cut to `max_chars` with an
/// ellipsis. For text an app wrote and the shell will show.
pub fn tidy_to(text: &str, max_chars: usize, layout: Layout) -> String {
    let keep = |c: char| layout == Layout::Paragraphs && c == '\n';
    let clean: String = text
        .chars()
        .filter(|c| keep(*c) || !hidden_or_reordering(*c))
        .collect();
    let clean = clean.trim();
    if clean.chars().count() <= max_chars {
        return clean.to_owned();
    }
    let mut cut: String = clean.chars().take(max_chars.saturating_sub(1)).collect();
    cut.push('\u{2026}');
    cut
}

/// An app's name on one line, cut to [`MAX_LABEL_CHARS`].
pub fn tidy(text: &str) -> String {
    tidy_to(text, MAX_LABEL_CHARS, Layout::OneLine)
}

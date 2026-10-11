//! Where restore tokens are kept between runs.

use std::path::PathBuf;
use std::sync::Mutex;

use transom_core::{Grant, RestoreBook, RestoreToken};

use crate::chooser::source_from_wire;

/// Why the book could not be read or written.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StoreError {
    /// The file could not be read or written.
    #[error("the restore file: {0}")]
    Io(#[from] std::io::Error),
    /// The file is not in the format this version writes.
    #[error("the restore file is damaged")]
    Damaged,
}

/// The seam: load the book at start and save it after a change.
pub trait RestoreStore: Send + Sync + std::fmt::Debug {
    /// The saved book; an empty one if nothing was saved.
    fn load(&self) -> Result<RestoreBook, StoreError>;
    /// Replace the saved book.
    fn save(&self, book: &RestoreBook) -> Result<(), StoreError>;
}

/// A store in a file the caller names (a file in the user's state directory, mode 0600, because
/// a token is a secret).
#[derive(Debug)]
pub struct FileStore {
    path: PathBuf,
}

impl FileStore {
    /// A store at `path`.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Where this store reads and writes.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl RestoreStore for FileStore {
    fn load(&self) -> Result<RestoreBook, StoreError> {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => decode(&text),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(RestoreBook::new()),
            // A file that is not text is a file we did not write.
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
                Err(StoreError::Damaged)
            }
            Err(error) => Err(StoreError::Io(error)),
        }
    }

    fn save(&self, book: &RestoreBook) -> Result<(), StoreError> {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;

        if let Some(dir) = self.path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir)?;
        }
        let beside = self.beside();
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&beside)?;
        file.write_all(encode(book).as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&beside, &self.path)?;
        Ok(())
    }
}

impl FileStore {
    /// The file a save is written to before it replaces the real one.
    fn beside(&self) -> PathBuf {
        let mut name = self.path.file_name().unwrap_or_default().to_os_string();
        name.push(".new");
        self.path.with_file_name(name)
    }
}

/// The first line of the file; a different one is a file this version did not write.
const HEADER: &str = "transom-restore 1";

/// One grant per line: the token, the app, then each source as `kind:name`, tab separated. The
/// fields escape backslash, tab, newline and carriage return, so a window identifier can hold
/// anything.
fn encode(book: &RestoreBook) -> String {
    let mut text = String::from(HEADER);
    text.push('\n');
    for (token, grant) in book.iter() {
        text.push_str(token.expose());
        text.push('\t');
        text.push_str(&escape(&grant.app_id));
        for source in &grant.sources {
            let (kind, name) = source.wire();
            text.push('\t');
            text.push_str(kind);
            text.push(':');
            text.push_str(&escape(name));
        }
        text.push('\n');
    }
    text
}

fn decode(text: &str) -> Result<RestoreBook, StoreError> {
    let mut lines = text.lines();
    if lines.next() != Some(HEADER) {
        return Err(StoreError::Damaged);
    }
    let mut book = RestoreBook::new();
    for line in lines.filter(|l| !l.is_empty()) {
        let mut fields = line.split('\t');
        let token = fields
            .next()
            .and_then(RestoreToken::parse)
            .ok_or(StoreError::Damaged)?;
        let app_id = fields
            .next()
            .and_then(unescape)
            .ok_or(StoreError::Damaged)?;
        let sources = fields
            .map(|field| {
                let (kind, name) = field.split_once(':')?;
                source_from_wire(kind, &unescape(name)?)
            })
            .collect::<Option<Vec<_>>>()
            .ok_or(StoreError::Damaged)?;
        book.remember(token, Grant { app_id, sources });
    }
    Ok(book)
}

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out
}

/// The text back, or `None` for an escape this version never writes.
fn unescape(text: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        out.push(match chars.next()? {
            '\\' => '\\',
            't' => '\t',
            'n' => '\n',
            'r' => '\r',
            _ => return None,
        });
    }
    Some(out)
}

/// A store that lives in memory, for tests.
#[derive(Debug, Default)]
pub struct MemoryStore {
    book: Mutex<RestoreBook>,
}

impl RestoreStore for MemoryStore {
    fn load(&self) -> Result<RestoreBook, StoreError> {
        Ok(self.book.lock().map(|b| b.clone()).unwrap_or_default())
    }

    fn save(&self, book: &RestoreBook) -> Result<(), StoreError> {
        if let Ok(mut held) = self.book.lock() {
            *held = book.clone();
        }
        Ok(())
    }
}

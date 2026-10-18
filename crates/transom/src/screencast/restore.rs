//! Where restore tokens are kept between runs.

use std::path::PathBuf;
use std::sync::Mutex;

use transom_core::{Grant, Keep, RestoreBook, RestoreToken};

use crate::chooser::source_from_wire;
use crate::fsread::read_text;

/// The largest restore file that is read. Far more than [`transom_core::MAX_GRANTS`] grants need;
/// a bigger file is not one this program wrote.
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

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
    /// The saved book. A file whose first line is not this version's is moved aside to
    /// `<name>.bad` and reported as damaged; a line that cannot be read is skipped (loudly) and
    /// a copy of the whole file is kept as `<name>.bad`, so a flaw in one line loses one grant and
    /// never the rest, and nothing is lost without a trace.
    fn load(&self) -> Result<RestoreBook, StoreError> {
        let text = match read_text(&self.path, MAX_FILE_BYTES) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(RestoreBook::new());
            }
            // A file that is not text, not a regular file or far too large is not one we wrote.
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::InvalidData | std::io::ErrorKind::InvalidInput
                ) =>
            {
                self.set_aside(Aside::Move);
                return Err(StoreError::Damaged);
            }
            Err(error) => return Err(StoreError::Io(error)),
        };
        match decode(&text) {
            Decoded::Book { book, skipped: 0 } => Ok(book),
            Decoded::Book { book, skipped } => {
                tracing::error!(
                    skipped,
                    "lines of the restore file could not be read and were left out; \
                     a copy of the file is kept next to it"
                );
                self.set_aside(Aside::Copy);
                Ok(book)
            }
            Decoded::OlderVersion => {
                tracing::info!("the restore file is from an older version; its grants are dropped");
                Ok(RestoreBook::new())
            }
            Decoded::Foreign => {
                self.set_aside(Aside::Move);
                Err(StoreError::Damaged)
            }
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
        // The rename is only on disk once the directory entry is: without this a power cut right
        // after a save can bring the old book back.
        if let Some(dir) = self.path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::File::open(dir)?.sync_all()?;
        }
        Ok(())
    }
}

/// What to do with a file that could not be fully read.
#[derive(Clone, Copy, Debug)]
enum Aside {
    /// Move it away: it is not ours, and the next save must not replace it unseen.
    Move,
    /// Keep a copy: it is ours, with a flaw.
    Copy,
}

impl FileStore {
    /// The file a save is written to before it replaces the real one.
    fn beside(&self) -> PathBuf {
        self.with_suffix(".new")
    }

    /// Where a file that could not be read is kept.
    pub fn bad_path(&self) -> PathBuf {
        self.with_suffix(".bad")
    }

    fn with_suffix(&self, suffix: &str) -> PathBuf {
        let mut name = self.path.file_name().unwrap_or_default().to_os_string();
        name.push(suffix);
        self.path.with_file_name(name)
    }

    fn set_aside(&self, how: Aside) {
        let bad = self.bad_path();
        let done = match how {
            Aside::Move => std::fs::rename(&self.path, &bad),
            Aside::Copy => std::fs::copy(&self.path, &bad).map(|_| ()),
        };
        if let Err(error) = done {
            tracing::error!(%error, "the damaged restore file could not be kept");
        }
    }
}

/// The first line of the file; a different one is a file this version did not write.
const HEADER: &str = "transom-restore 2";

/// The first line of the version before this one, which kept no mode and no date.
const OLDER_HEADER: &str = "transom-restore 1";

/// One grant per line: the token, the app, the persist mode (1 or 2), when it was given (seconds
/// since the epoch), then each source as `kind:name`, tab separated. The fields escape backslash,
/// tab, newline and carriage return, so a window identifier can hold anything.
fn encode(book: &RestoreBook) -> String {
    let mut text = String::from(HEADER);
    text.push('\n');
    for (token, grant) in book.iter() {
        text.push_str(token.expose());
        text.push('\t');
        text.push_str(&escape(&grant.app_id));
        text.push('\t');
        text.push_str(&grant.keep.to_wire().to_string());
        text.push('\t');
        text.push_str(&grant.granted_at.to_string());
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

/// What a file's text turned out to be.
enum Decoded {
    /// This version's file: the grants that could be read and how many lines could not.
    Book { book: RestoreBook, skipped: usize },
    /// The previous version's file.
    OlderVersion,
    /// Not a file this program wrote.
    Foreign,
}

fn decode(text: &str) -> Decoded {
    let mut lines = text.lines();
    match lines.next() {
        Some(HEADER) => {}
        Some(OLDER_HEADER) => return Decoded::OlderVersion,
        _ => return Decoded::Foreign,
    }
    let mut book = RestoreBook::new();
    let mut skipped = 0;
    for line in lines.filter(|l| !l.is_empty()) {
        match decode_line(line) {
            Some((token, grant)) => book.remember(token, grant),
            None => skipped += 1,
        }
    }
    Decoded::Book { book, skipped }
}

fn decode_line(line: &str) -> Option<(RestoreToken, Grant)> {
    let mut fields = line.split('\t');
    let token = RestoreToken::parse(fields.next()?)?;
    let app_id = unescape(fields.next()?)?;
    let keep = fields.next()?.parse().ok().and_then(Keep::from_wire)?;
    let granted_at = fields.next()?.parse().ok()?;
    let sources = fields
        .map(|field| {
            let (kind, name) = field.split_once(':')?;
            source_from_wire(kind, &unescape(name)?)
        })
        .collect::<Option<Vec<_>>>()?;
    Some((
        token,
        Grant {
            app_id,
            sources,
            keep,
            granted_at,
        },
    ))
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

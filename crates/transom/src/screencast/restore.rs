//! Where restore tokens are kept between runs.

use std::path::PathBuf;
use std::sync::Mutex;

use transom_core::RestoreBook;

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
        let _ = &self.path;
        todo!("P2-T2: parse the line format; a missing file is an empty book")
    }

    fn save(&self, book: &RestoreBook) -> Result<(), StoreError> {
        let _ = (&self.path, book);
        todo!("P2-T2: write beside the file, mode 0600, rename over it")
    }
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

//! The book of what the person let apps do without asking again.
//!
//! Screen sharing and remote control remember in one book, in one file, behind one lock, so the
//! list Settings shows and the one button that takes an app's access back cover both. Reading and
//! writing the file blocks, so the async methods run it on a blocking thread.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use transom_core::{
    CastSource, DeviceKinds, Grant, InputGrant, Keep, Remembered, RestoreBook, RestoreToken,
};

use crate::screencast::restore::{RestoreStore, StoreError};

/// What a grant to remember is for.
#[derive(Clone, Debug)]
pub enum Kept {
    /// These shared sources.
    Screen(Vec<CastSource>),
    /// These input kinds.
    Input(DeviceKinds),
}

/// A grant that may be kept, of either kind.
enum Issued {
    Screen(Grant),
    Input(InputGrant),
}

/// The restore store and the lock that keeps a change of the book whole.
#[derive(Debug)]
pub struct Keeper {
    store: Box<dyn RestoreStore>,
    /// Held while the book is read, changed and written, so two changes do not lose a token.
    lock: Mutex<()>,
}

impl Keeper {
    /// A keeper over `store`.
    pub fn new(store: Box<dyn RestoreStore>) -> Self {
        Self {
            store,
            lock: Mutex::new(()),
        }
    }

    fn held(&self) -> MutexGuard<'_, ()> {
        self.lock.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The saved book; an empty one if it cannot be read.
    pub async fn book(self: &Arc<Self>) -> RestoreBook {
        let this = Arc::clone(self);
        tokio::task::spawn_blocking(move || this.read())
            .await
            .unwrap_or_default()
    }

    fn read(&self) -> RestoreBook {
        let _held = self.held();
        self.store.load().unwrap_or_else(|error| {
            tracing::warn!(%error, "the restore tokens could not be read");
            RestoreBook::new()
        })
    }

    /// Remember `kept` for `app_id` under a fresh token (and forget the one it replaces). `None`
    /// when nothing may be kept (the app has no identity, or was given nothing), or no token could
    /// be made or kept; the person is then asked again next time.
    pub async fn keep(
        self: &Arc<Self>,
        app_id: &str,
        kept: Kept,
        keep: Keep,
        replaces: Option<RestoreToken>,
    ) -> Option<RestoreToken> {
        let (this, app_id) = (Arc::clone(self), app_id.to_owned());
        tokio::task::spawn_blocking(move || this.keep_now(&app_id, kept, keep, replaces.as_ref()))
            .await
            .ok()
            .flatten()
    }

    fn keep_now(
        &self,
        app_id: &str,
        kept: Kept,
        keep: Keep,
        replaces: Option<&RestoreToken>,
    ) -> Option<RestoreToken> {
        let now = unix_now();
        let issued = match kept {
            Kept::Screen(sources) => Grant::issue(app_id, sources, keep, now).map(Issued::Screen),
            Kept::Input(devices) => {
                InputGrant::issue(app_id, devices, keep, now).map(Issued::Input)
            }
        };
        let Some(issued) = issued else {
            tracing::info!(
                "this access is not remembered: the app has no identity to remember it by"
            );
            return None;
        };
        let token = RestoreToken::mint(crate::random::bytes::<16>()?);
        let _held = self.held();
        let mut book = match self.store.load() {
            Ok(book) => book,
            // The damaged file was set aside by the store, so nothing is lost by starting anew.
            Err(StoreError::Damaged) => {
                tracing::error!("the saved grants were damaged and were set aside; starting anew");
                RestoreBook::new()
            }
            // Contents unknown: saving now could erase everyone else's grants.
            Err(error) => {
                tracing::error!(%error, "the saved grants could not be read, so this one is not saved");
                return None;
            }
        };
        if let Some(old) = replaces {
            book.forget(old);
        }
        match issued {
            Issued::Screen(grant) => book.remember(token.clone(), grant),
            Issued::Input(grant) => book.remember_input(token.clone(), grant),
        }
        book.prune(now);
        match self.store.save(&book) {
            Ok(()) => Some(token),
            Err(error) => {
                tracing::warn!(%error, "the restore tokens could not be saved");
                None
            }
        }
    }

    /// The apps the person has let in without asking again.
    pub async fn remembered(self: &Arc<Self>) -> Vec<Remembered> {
        let this = Arc::clone(self);
        tokio::task::spawn_blocking(move || this.read().remembered(unix_now()))
            .await
            .unwrap_or_default()
    }

    /// Take back everything remembered for `app_id`, of either kind; how many grants went, or
    /// `None` when the saved book could not be read or written (nothing is then changed).
    pub async fn revoke(self: &Arc<Self>, app_id: &str) -> Option<usize> {
        let (this, app_id) = (Arc::clone(self), app_id.to_owned());
        tokio::task::spawn_blocking(move || this.revoke_now(&app_id))
            .await
            .ok()
            .flatten()
    }

    fn revoke_now(&self, app_id: &str) -> Option<usize> {
        let _held = self.held();
        let mut book = match self.store.load() {
            Ok(book) => book,
            // Nothing readable is nothing remembered; the store set a damaged file aside.
            Err(StoreError::Damaged) => return Some(0),
            Err(error) => {
                tracing::error!(%error, "the saved grants could not be read, so none was taken back");
                return None;
            }
        };
        let gone = book.forget_app(app_id);
        if gone == 0 {
            return Some(0);
        }
        match self.store.save(&book) {
            Ok(()) => Some(gone),
            Err(error) => {
                tracing::error!(%error, "the saved grants could not be written, so none was taken back");
                None
            }
        }
    }
}

/// Seconds since the Unix epoch, the clock grants are dated by.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

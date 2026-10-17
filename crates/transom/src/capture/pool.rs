//! Memory for captured pictures that is used again instead of allocated for every frame
//! (PORTAL-12). A 4K picture is 33 MB; at the stream's pace, allocating and freeing one per frame
//! asks the system for gigabytes of fresh pages every second.
//!
//! The capture takes a buffer from the pool, fills it, and hands it on as a [`PixelBuffer`]. Wherever
//! the picture finally stops being held (the sink replaced it, the stream ended) the buffer
//! drops and goes back. The pool keeps a few at most ([`POOL_KEEP`]) and none that are far
//! larger than what is asked for, so a share that changes size does not keep the old memory.

use std::fmt;
use std::ops::Deref;
use std::sync::{Arc, Mutex, PoisonError};

/// The most idle buffers a pool keeps: one being filled, one held by the sink as the picture to
/// repeat, and one the stream is still reading.
pub const POOL_KEEP: usize = 3;

/// A pooled buffer larger than this many times what is asked for is let go instead of reused: the
/// source shrank, and the old memory is not worth keeping.
const OVERSIZE_FACTOR: usize = 2;

/// Buffers waiting to be used again.
#[derive(Debug, Default)]
pub struct FramePool {
    idle: Mutex<Vec<Vec<u8>>>,
}

impl FramePool {
    /// An empty pool.
    pub fn new() -> Arc<Self> {
        Arc::default()
    }

    /// A buffer of exactly `len` bytes, zeroed, from the pool when one fits; it returns to this
    /// pool when the [`PixelBuffer`] is dropped.
    pub fn take(self: &Arc<Self>, len: usize) -> PixelBuffer {
        let reused = self
            .idle
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop()
            .filter(|bytes| bytes.capacity() <= len.saturating_mul(OVERSIZE_FACTOR));
        let mut bytes = reused.unwrap_or_default();
        bytes.clear();
        bytes.resize(len, 0);
        PixelBuffer {
            bytes,
            home: Some(Arc::clone(self)),
        }
    }

    /// How many buffers are waiting.
    pub fn idle(&self) -> usize {
        self.idle
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    fn give(&self, bytes: Vec<u8>) {
        let mut idle = self.idle.lock().unwrap_or_else(PoisonError::into_inner);
        if idle.len() < POOL_KEEP && bytes.capacity() > 0 {
            idle.push(bytes);
        }
    }
}

/// The pixels of a picture. Read it as a slice; when it drops, a pooled one goes back to its pool.
pub struct PixelBuffer {
    bytes: Vec<u8>,
    home: Option<Arc<FramePool>>,
}

impl PixelBuffer {
    /// The bytes, to fill them in place.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.bytes
    }
}

impl From<Vec<u8>> for PixelBuffer {
    /// A buffer that belongs to no pool: it is freed when dropped.
    fn from(bytes: Vec<u8>) -> Self {
        Self { bytes, home: None }
    }
}

impl Deref for PixelBuffer {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.bytes
    }
}

impl fmt::Debug for PixelBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PixelBuffer({} bytes)", self.bytes.len())
    }
}

impl Drop for PixelBuffer {
    fn drop(&mut self) {
        if let Some(home) = self.home.take() {
            home.give(std::mem::take(&mut self.bytes));
        }
    }
}

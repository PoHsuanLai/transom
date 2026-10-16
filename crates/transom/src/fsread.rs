//! Reading a small file that somebody else may have replaced with something hostile.
//!
//! A path in the person's home can be made into a FIFO (a read would block for ever), a device, or
//! a file of any size. [`read_text`] opens without blocking, insists on a regular file, and gives
//! up past a size it is told. It still blocks on a slow disk, so async callers run it on a blocking
//! thread.

use std::io::{self, Read};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// The text of the regular file at `path`, at most `max_bytes` long.
///
/// A file that is not a regular file is `InvalidInput`; one longer than `max_bytes` is
/// `InvalidData`, as is one that is not UTF-8. A missing file keeps its `NotFound`.
pub(crate) fn read_text(path: &Path, max_bytes: u64) -> io::Result<String> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        // So that opening a FIFO with no writer returns at once instead of waiting for one.
        .custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a regular file",
        ));
    }
    if meta.len() > max_bytes {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "file too large"));
    }
    let mut text = String::new();
    // One byte more than allowed, so a file that grew since `metadata` is caught too.
    file.take(max_bytes.saturating_add(1))
        .read_to_string(&mut text)?;
    if u64::try_from(text.len()).is_ok_and(|len| len > max_bytes) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "file too large"));
    }
    Ok(text)
}

//! Waiting on the Wayland socket for a bounded time.

use std::os::fd::BorrowedFd;
use std::time::Duration;

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::io::Errno;

use super::{CaptureError, broken};

/// What a bounded wait on the socket came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Waited {
    /// The condition held.
    Done,
    /// The time ran out first.
    TimedOut,
}

/// Whether the socket has something to read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Readiness {
    /// Data (or an error to be read) is waiting, or the wait was interrupted.
    Ready,
    /// The time ran out.
    Silent,
}

/// Wait up to `wait` for `fd` to become readable.
pub(super) fn readable_within(
    fd: BorrowedFd<'_>,
    wait: Duration,
) -> Result<Readiness, CaptureError> {
    let mut fds = [PollFd::new(&fd, PollFlags::IN | PollFlags::ERR)];
    let timeout = Timespec {
        tv_sec: i64::try_from(wait.as_secs()).unwrap_or(i64::MAX),
        tv_nsec: i64::from(wait.subsec_nanos()),
    };
    match poll(&mut fds, Some(&timeout)) {
        Ok(0) => Ok(Readiness::Silent),
        Ok(_) | Err(Errno::INTR) => Ok(Readiness::Ready),
        Err(error) => Err(broken(error)),
    }
}

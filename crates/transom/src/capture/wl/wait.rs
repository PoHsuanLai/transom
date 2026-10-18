//! Waiting on the Wayland socket for a bounded time.

use std::os::fd::BorrowedFd;
use std::sync::mpsc;
use std::time::Duration;

use rustix::event::{PollFd, PollFlags, Timespec, poll};
use rustix::io::Errno;

use super::{Canceller, CaptureError, Connection, broken};

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

/// Cuts a Wayland connection if a call that cannot be given a deadline of its own takes too long.
///
/// Arm it before the call and drop it after: dropping disarms it. If the deadline passes first the
/// connection is shut, which makes the blocked call return an error.
#[derive(Debug)]
pub(super) struct Watchdog {
    /// Dropping this wakes the watchdog thread, which then ends without cutting anything.
    _disarm: mpsc::Sender<()>,
}

impl Watchdog {
    /// Watch `conn` for `deadline`. `None` if no thread could be started (the call then has no
    /// deadline, as it had none before).
    pub(super) fn arm(conn: &Connection, deadline: Duration) -> Option<Self> {
        let fd = conn.backend().poll_fd().try_clone_to_owned().ok()?;
        let canceller = Canceller::new(fd);
        let (disarm, armed) = mpsc::channel::<()>();
        std::thread::Builder::new()
            .name("transom-watchdog".to_owned())
            .spawn(move || {
                if armed.recv_timeout(deadline) == Err(mpsc::RecvTimeoutError::Timeout) {
                    canceller.cancel();
                }
            })
            .ok()?;
        Some(Self { _disarm: disarm })
    }
}

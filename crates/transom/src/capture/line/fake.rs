//! A line for tests: it makes socket pairs and records how often it was asked.

use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::sync::Mutex;
use std::sync::PoisonError;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{LineError, PortalLine};

/// What a fake line answers.
#[derive(Debug)]
enum Answer {
    Grant,
    Unavailable,
    Refuse,
    TimeOut,
}

/// Answers every ask the same way, and keeps the server end of each connection it grants.
#[derive(Debug)]
pub struct FakeLine {
    answer: Answer,
    asked: AtomicUsize,
    servers: Mutex<Vec<UnixStream>>,
}

impl FakeLine {
    fn answering(answer: Answer) -> Self {
        Self {
            answer,
            asked: AtomicUsize::new(0),
            servers: Mutex::new(Vec::new()),
        }
    }

    /// A line that grants a connection each time.
    pub fn granting() -> Self {
        Self::answering(Answer::Grant)
    }

    /// A line with nobody on the bus to ask.
    pub fn unowned() -> Self {
        Self::answering(Answer::Unavailable)
    }

    /// A line the compositor refuses.
    pub fn refusing() -> Self {
        Self::answering(Answer::Refuse)
    }

    /// A line the compositor never answers.
    pub fn silent() -> Self {
        Self::answering(Answer::TimeOut)
    }

    /// How many times it was asked.
    pub fn asked(&self) -> usize {
        self.asked.load(Ordering::SeqCst)
    }

    /// Takes the server end of the most recent connection granted.
    pub fn take_server(&self) -> Option<UnixStream> {
        self.servers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop()
    }
}

impl PortalLine for FakeLine {
    fn open(&self) -> Result<OwnedFd, LineError> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        match self.answer {
            Answer::Grant => {
                let (server, client) =
                    UnixStream::pair().map_err(|error| LineError::Refused(error.to_string()))?;
                self.servers
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(server);
                Ok(OwnedFd::from(client))
            }
            Answer::Unavailable => Err(LineError::Unavailable),
            Answer::Refuse => Err(LineError::Refused("not the portal".to_owned())),
            Answer::TimeOut => Err(LineError::TimedOut),
        }
    }
}

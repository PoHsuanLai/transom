//! The portal's reply: a response code and a dictionary of results.

use crate::screencast::Dict;

/// How a portal method ended, before it becomes the `(u32, a{sv})` pair on the bus.
#[derive(Debug)]
pub enum Response {
    /// It worked; the reply's results.
    Granted(Dict),
    /// The person said no, or closed the question.
    Cancelled,
    /// Something broke.
    Failed,
}

impl Response {
    /// The wire code of [`Response::Granted`].
    pub const GRANTED: u32 = 0;
    /// The wire code of [`Response::Cancelled`].
    pub const CANCELLED: u32 = 1;
    /// The wire code of [`Response::Failed`].
    pub const FAILED: u32 = 2;

    /// A success with no results.
    pub fn granted() -> Self {
        Self::Granted(Dict::new())
    }

    /// The portal's response code and results: 0 granted, 1 cancelled, 2 failed.
    pub fn into_wire(self) -> (u32, Dict) {
        match self {
            Self::Granted(results) => (Self::GRANTED, results),
            Self::Cancelled => (Self::CANCELLED, Dict::new()),
            Self::Failed => (Self::FAILED, Dict::new()),
        }
    }
}

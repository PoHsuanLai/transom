//! One capture job's way in.

use std::sync::{Arc, OnceLock};

use transom_core::{CastSource, OutputFacts};

use super::wl::{Link, Wire};
use super::{CaptureError, DisplayName};

/// One job of capturing: a screenshot, the lookups and streams of one screen-cast start. The
/// compositor limits how often one caller may ask for a connection, so a job asks for one, on its
/// first step, and runs every step on it: listing the sources, reading the outputs, and each
/// output or window it captures ([`ExtCapture::open`](super::ExtCapture::open)). Where the
/// compositor hands none out, the session's own display is used instead, and that is likewise
/// decided once for the job, not once per step. A capture that goes on running (a share) keeps
/// the connection open for as long as it runs.
#[derive(Debug)]
pub struct Job {
    display: DisplayName,
    /// The connection, or why it could not be had: a job that failed to connect fails the same
    /// way at its next step instead of asking again.
    wire: OnceLock<Result<Arc<Wire>, String>>,
}

impl Job {
    /// A job on `display`. Nothing is asked of the compositor until the first step.
    pub fn new(display: &DisplayName) -> Self {
        Self {
            display: display.clone(),
            wire: OnceLock::new(),
        }
    }

    /// Every output and window that can be captured now, as the compositor lists them.
    pub fn live_sources(&self) -> Result<Vec<CastSource>, CaptureError> {
        self.link()?.live_sources()
    }

    /// Every output the compositor lists now: its name, place in the logical space, mode, turn and
    /// scale. The compositor is asked afresh, so the list is current when it returns.
    pub fn output_facts(&self) -> Result<Vec<OutputFacts>, CaptureError> {
        self.link()?.output_facts()
    }

    /// A session of this job's own on its connection; the connection is made on the first call.
    pub(super) fn link(&self) -> Result<Link, CaptureError> {
        self.wire
            .get_or_init(|| Wire::open(&self.display).map_err(failed))
            .as_ref()
            .map(Wire::link)
            .map_err(|why| CaptureError::Connection(why.clone()))
    }
}

/// What to say of a connection that could not be made, without saying "connection" twice.
fn failed(error: CaptureError) -> String {
    match error {
        CaptureError::Connection(why) => why,
        other => other.to_string(),
    }
}

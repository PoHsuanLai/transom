//! A share that is running: for each source a capture, a PipeWire stream and the thread that
//! moves frames from one to the other.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::thread::JoinHandle;

use transom_core::{CastSource, CursorMode, PwNode};

use super::PipewireSink;
use super::options::StreamInfo;
use super::stream::{FrameSink, Pump, PumpOutcome};
use crate::capture::{CaptureError, CaptureSource, DisplayName, ExtCapture};

/// Why a share could not be opened.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum OpenError {
    /// A source could not be captured.
    #[error("capture: {0}")]
    Capture(#[from] CaptureError),
    /// A stream could not be created.
    #[error("stream: {0}")]
    Stream(#[from] super::stream::SinkError),
    /// A thread could not be started.
    #[error("thread: {0}")]
    Thread(#[from] std::io::Error),
    /// Nothing was asked for.
    #[error("no sources")]
    NoSources,
}

/// How a stream's thread ended.
const RUNNING: u8 = 0;
const STOPPED_BY_US: u8 = 1;

/// One source being shared.
#[derive(Debug)]
struct Running {
    info: StreamInfo,
    state: Arc<AtomicU8>,
    thread: Option<JoinHandle<()>>,
}

/// Every stream of one session. Dropping it stops them all.
#[derive(Debug)]
pub struct Share {
    streams: Vec<Running>,
}

impl Share {
    /// Open a capture and a PipeWire stream for each source and start moving frames.
    ///
    /// Blocks while connecting; call it off the async runtime. `ended` runs, on the stream's own
    /// thread, when a source goes away by itself (a window closed, a display unplugged); it does
    /// not run for a share that was dropped.
    pub fn open(
        display: &DisplayName,
        sources: &[CastSource],
        cursor: CursorMode,
        ended: impl Fn() + Send + Clone + 'static,
    ) -> Result<Self, OpenError> {
        if sources.is_empty() {
            return Err(OpenError::NoSources);
        }
        // A failure part way drops what was built, which stops it.
        let mut share = Self {
            streams: Vec::new(),
        };
        for source in sources {
            share
                .streams
                .push(open_one(display, source, cursor, ended.clone())?);
        }
        Ok(share)
    }

    /// The streams, for the reply.
    pub fn infos(&self) -> Vec<StreamInfo> {
        self.streams.iter().map(|s| s.info.clone()).collect()
    }

    /// The first stream's node, the one the session machine reports.
    pub fn first_node(&self) -> Option<PwNode> {
        self.streams.first().map(|s| s.info.node)
    }
}

impl Drop for Share {
    fn drop(&mut self) {
        for stream in &self.streams {
            stream.state.store(STOPPED_BY_US, Ordering::SeqCst);
        }
        // Each thread looks at its flag after every quiet turn (a quarter of a second at most)
        // and ends by itself, closing its own connection; they are not waited for here.
        for stream in &mut self.streams {
            drop(stream.thread.take());
        }
    }
}

fn open_one(
    display: &DisplayName,
    source: &CastSource,
    cursor: CursorMode,
    ended: impl Fn() + Send + 'static,
) -> Result<Running, OpenError> {
    let capture = ExtCapture::open_with(display, source, cursor)?;
    let size = capture.size();
    let sink = PipewireSink::open(size, cursor)?;
    let info = StreamInfo {
        node: sink.node(),
        source: source.clone(),
        size,
    };
    let state = Arc::new(AtomicU8::new(RUNNING));
    let mut pump = Pump::new(capture, sink, cursor);
    let thread = std::thread::Builder::new()
        .name("transom-share".to_owned())
        .spawn({
            let state = Arc::clone(&state);
            move || {
                while state.load(Ordering::SeqCst) == RUNNING
                    && let Ok(
                        PumpOutcome::Pushed | PumpOutcome::ResizedAndPushed | PumpOutcome::Quiet,
                    ) = pump.turn()
                {}
                drop(pump);
                if state.load(Ordering::SeqCst) == RUNNING {
                    ended();
                }
            }
        })?;
    Ok(Running {
        info,
        state,
        thread: Some(thread),
    })
}

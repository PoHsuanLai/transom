//! Moving frames from a [`CaptureSource`] to a [`FrameSink`].
//!
//! The sink is the seam in front of PipeWire: the real one is [`super::pipewire::PipewireSink`],
//! [`FakeSink`] records what it was given. [`Pump`] is the whole loop body, so the logic between
//! them is tested without either.

use transom_core::{CursorMode, PwNode};

use crate::capture::{CaptureError, CaptureSource, CapturedFrame, PhysicalSizeWire};

/// Why a frame could not be handed on.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SinkError {
    /// No app is reading the stream any more.
    #[error("nobody is reading the stream")]
    NoReader,
    /// PipeWire failed.
    #[error("pipewire: {0}")]
    Pipewire(String),
}

/// Where frames go.
pub trait FrameSink: Send + std::fmt::Debug {
    /// The node apps connect to.
    fn node(&self) -> PwNode;
    /// Offer one frame. A sink with no free buffer drops the frame and returns `Ok`.
    fn push(&mut self, frame: CapturedFrame) -> Result<(), SinkError>;
    /// Tell the sink the stream's size changed (the window was resized).
    fn resize(&mut self, size: PhysicalSizeWire) -> Result<(), SinkError>;
}

/// What one turn of the pump did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum PumpOutcome {
    /// A frame went to the sink.
    Pushed,
    /// The sink had to be resized first, then a frame went.
    ResizedAndPushed,
    /// The source is gone; the stream is over.
    SourceEnded,
}

/// The loop body: take a frame from the source, give it to the sink.
#[derive(Debug)]
pub struct Pump<C, S> {
    source: C,
    sink: S,
    last_size: PhysicalSizeWire,
    cursor: CursorMode,
}

impl<C: CaptureSource, S: FrameSink> Pump<C, S> {
    /// A pump between `source` and `sink`.
    pub fn new(source: C, sink: S, cursor: CursorMode) -> Self {
        let last_size = source.size();
        Self {
            source,
            sink,
            last_size,
            cursor,
        }
    }

    /// The node the sink serves.
    pub fn node(&self) -> PwNode {
        self.sink.node()
    }

    /// The cursor mode this stream was set up with.
    pub fn cursor(&self) -> CursorMode {
        self.cursor
    }

    /// Take the sink back (to inspect a fake, or to close a real one).
    pub fn into_sink(self) -> S {
        self.sink
    }

    /// One turn: block for a frame, resize the sink if the frame's size changed, push it.
    pub fn turn(&mut self) -> Result<PumpOutcome, SinkError> {
        let frame = match self.source.frame() {
            Ok(frame) => frame,
            Err(CaptureError::Ended | CaptureError::Stopped) => {
                return Ok(PumpOutcome::SourceEnded);
            }
            Err(other) => return Err(SinkError::Pipewire(other.to_string())),
        };
        let resized = frame.size != self.last_size;
        if resized {
            self.sink.resize(frame.size)?;
            self.last_size = frame.size;
        }
        self.sink.push(frame)?;
        Ok(if resized {
            PumpOutcome::ResizedAndPushed
        } else {
            PumpOutcome::Pushed
        })
    }
}

/// A sink that keeps what it was given.
#[derive(Debug)]
pub struct FakeSink {
    node: PwNode,
    /// The presentation stamps of the frames received, in order.
    pub received: Vec<u64>,
    /// The sizes the sink was resized to, in order.
    pub resizes: Vec<PhysicalSizeWire>,
}

impl FakeSink {
    /// A sink serving `node`.
    pub fn new(node: PwNode) -> Self {
        Self {
            node,
            received: Vec::new(),
            resizes: Vec::new(),
        }
    }
}

impl FrameSink for FakeSink {
    fn node(&self) -> PwNode {
        self.node
    }

    fn push(&mut self, frame: CapturedFrame) -> Result<(), SinkError> {
        self.received.push(frame.presented_ns);
        Ok(())
    }

    fn resize(&mut self, size: PhysicalSizeWire) -> Result<(), SinkError> {
        self.resizes.push(size);
        Ok(())
    }
}

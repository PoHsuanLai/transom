//! The PipeWire side: a video source stream apps connect to, fed by a [`FrameSink`].
//!
//! Frames are offered as dmabuf when the consumer negotiates it and as shared memory otherwise.
//! The cursor is drawn into the picture, left out, or sent as stream metadata according to the
//! session's [`CursorMode`].

use transom_core::{CursorMode, PwNode};

use super::stream::{FrameSink, SinkError};
use crate::capture::{CapturedFrame, PhysicalSizeWire};

/// A live PipeWire stream.
#[derive(Debug)]
pub struct PipewireSink {
    node: PwNode,
}

impl PipewireSink {
    /// Create the stream at `size` and wait until PipeWire has given it a node id (P2-T2).
    pub fn open(size: PhysicalSizeWire, cursor: CursorMode) -> Result<Self, SinkError> {
        let _ = (size, cursor);
        todo!(
            "P2-T2: pipewire-rs main loop on its own thread, a Video/Source stream, format negotiation"
        )
    }
}

impl FrameSink for PipewireSink {
    fn node(&self) -> PwNode {
        self.node
    }

    fn push(&mut self, frame: CapturedFrame) -> Result<(), SinkError> {
        let _ = frame;
        todo!("P2-T2: dequeue a buffer, copy shm or attach the dmabuf, queue it")
    }

    fn resize(&mut self, size: PhysicalSizeWire) -> Result<(), SinkError> {
        let _ = size;
        todo!("P2-T2: renegotiate the format at the new size")
    }
}

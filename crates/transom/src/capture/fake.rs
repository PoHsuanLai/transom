//! A capture source for tests: serves frames it was given, then ends.

use std::collections::VecDeque;

use super::{CaptureError, CaptureSource, CapturedFrame, FrameData, PhysicalSizeWire, PixelFormat};

/// Serves a fixed run of frames, then [`CaptureError::Ended`].
#[derive(Debug)]
pub struct FakeCapture {
    size: PhysicalSizeWire,
    frames: VecDeque<CapturedFrame>,
}

impl FakeCapture {
    /// A source that will serve exactly these frames, in order.
    pub fn new(size: PhysicalSizeWire, frames: Vec<CapturedFrame>) -> Self {
        Self {
            size,
            frames: frames.into(),
        }
    }

    /// `count` solid shm frames of `size`; frame `n` is filled with the byte `n` (mod 256) and
    /// stamped `n` milliseconds, so a test can tell them apart.
    pub fn solid(size: PhysicalSizeWire, count: u32) -> Self {
        let stride = size.width * 4;
        let frames = (0..count)
            .map(|n| CapturedFrame {
                size,
                data: FrameData::Shm {
                    format: PixelFormat::Xrgb8888,
                    stride,
                    bytes: vec![(n % 256) as u8; stride as usize * size.height as usize],
                },
                presented_ns: u64::from(n) * 1_000_000,
            })
            .collect();
        Self::new(size, frames)
    }

    /// How many frames are still to come.
    pub fn remaining(&self) -> usize {
        self.frames.len()
    }
}

impl CaptureSource for FakeCapture {
    fn frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        self.frames.pop_front().ok_or(CaptureError::Ended)
    }

    fn size(&self) -> PhysicalSizeWire {
        self.size
    }
}

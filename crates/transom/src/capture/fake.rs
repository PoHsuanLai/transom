//! A capture source for tests: serves frames it was given, with quiet spells where it was told
//! to, then ends.

use std::collections::VecDeque;
use std::time::Duration;

use super::{
    Arrival, CaptureError, CaptureSource, CapturedFrame, FrameData, PhysicalSizeWire, PixelFormat,
};

/// Serves a fixed run of frames, then [`CaptureError::Ended`]. A `None` in the run is a spell in
/// which the picture did not change: [`CaptureSource::next_frame`] answers [`Arrival::Quiet`] once
/// for it, and [`CaptureSource::frame`] (which waits for a change) steps over it.
#[derive(Debug)]
pub struct FakeCapture {
    size: PhysicalSizeWire,
    frames: VecDeque<Option<CapturedFrame>>,
}

impl FakeCapture {
    /// A source that will serve exactly these frames, in order.
    pub fn new(size: PhysicalSizeWire, frames: Vec<CapturedFrame>) -> Self {
        Self::scripted(size, frames.into_iter().map(Some).collect())
    }

    /// A source that serves this run: a frame for each `Some`, a quiet spell for each `None`.
    pub fn scripted(size: PhysicalSizeWire, run: Vec<Option<CapturedFrame>>) -> Self {
        Self {
            size,
            frames: run.into(),
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
                    bytes: vec![(n % 256) as u8; stride as usize * size.height as usize].into(),
                },
                presented_ns: u64::from(n) * 1_000_000,
            })
            .collect();
        Self::new(size, frames)
    }

    /// How many frames are still to come.
    pub fn remaining(&self) -> usize {
        self.frames.iter().flatten().count()
    }
}

impl CaptureSource for FakeCapture {
    fn frame(&mut self) -> Result<CapturedFrame, CaptureError> {
        loop {
            match self.frames.pop_front() {
                Some(Some(frame)) => return Ok(frame),
                Some(None) => {}
                None => return Err(CaptureError::Ended),
            }
        }
    }

    fn next_frame(&mut self, wait: Duration) -> Result<Arrival, CaptureError> {
        let _ = wait;
        match self.frames.pop_front() {
            Some(Some(frame)) => Ok(Arrival::Frame(frame)),
            Some(None) => Ok(Arrival::Quiet),
            None => Err(CaptureError::Ended),
        }
    }

    fn size(&self) -> PhysicalSizeWire {
        self.size
    }
}

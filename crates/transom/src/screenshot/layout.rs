//! Which outputs a screenshot covers and where each one sits on the canvas.

use transom_core::{Tile, arrange};

use super::ShotError;
use crate::capture::Job;

/// The seam: which outputs exist and where they sit.
pub trait OutputLayout: Send + Sync + std::fmt::Debug {
    /// Every output as a tile (its place on the canvas, negative coordinates allowed, and the size it covers; a frame of another size is stretched to fit, so screens of different sharpness share one picture), asking on the connection of `job`.
    fn outputs(&self, job: &Job) -> Result<Vec<Tile>, ShotError>;
}

/// A layout given up front.
#[derive(Clone, Debug, Default)]
pub struct FixedLayout(pub Vec<Tile>);

impl OutputLayout for FixedLayout {
    fn outputs(&self, _job: &Job) -> Result<Vec<Tile>, ShotError> {
        Ok(self.0.clone())
    }
}

/// The layout the compositor reports now. Each call reads the outputs that exist at that moment,
/// so a screen plugged in or out, moved or rescaled since the last screenshot is accounted for
/// without any state to keep current.
#[derive(Clone, Copy, Debug)]
pub struct LiveLayout;

impl OutputLayout for LiveLayout {
    fn outputs(&self, job: &Job) -> Result<Vec<Tile>, ShotError> {
        let facts = job.output_facts()?;
        Ok(arrange(&facts))
    }
}

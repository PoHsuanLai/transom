//! Which outputs a screenshot covers and where each one sits on the canvas.

use transom_core::{Tile, arrange};

use super::ShotError;
use crate::capture::{Job, PhysicalSizeWire};

/// Where one output sits on the canvas, in pixels (the canvas's origin is the top-left of the
/// outputs together, so negative coordinates are fine), and the size it covers there. A frame
/// of another size is stretched to fit, so screens of different sharpness share one picture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputPlacement {
    /// The output's name, as the capture opens it.
    pub name: String,
    /// Left edge.
    pub x: i32,
    /// Top edge.
    pub y: i32,
    /// The size it covers.
    pub size: PhysicalSizeWire,
}

impl From<Tile> for OutputPlacement {
    fn from(tile: Tile) -> Self {
        Self {
            name: tile.name,
            x: tile.origin.x,
            y: tile.origin.y,
            size: PhysicalSizeWire {
                width: tile.size.width,
                height: tile.size.height,
            },
        }
    }
}

/// The seam: which outputs exist and where they sit.
pub trait OutputLayout: Send + Sync + std::fmt::Debug {
    /// Every output, with its place, asking on the connection of `job`.
    fn outputs(&self, job: &Job) -> Result<Vec<OutputPlacement>, ShotError>;
}

/// A layout given up front.
#[derive(Clone, Debug, Default)]
pub struct FixedLayout(pub Vec<OutputPlacement>);

impl OutputLayout for FixedLayout {
    fn outputs(&self, _job: &Job) -> Result<Vec<OutputPlacement>, ShotError> {
        Ok(self.0.clone())
    }
}

/// The layout the compositor reports now. Each call reads the outputs that exist at that moment,
/// so a screen plugged in or out, moved or rescaled since the last screenshot is accounted for
/// without any state to keep current.
#[derive(Clone, Copy, Debug)]
pub struct LiveLayout;

impl OutputLayout for LiveLayout {
    fn outputs(&self, job: &Job) -> Result<Vec<OutputPlacement>, ShotError> {
        let facts = job.output_facts()?;
        Ok(arrange(&facts)
            .into_iter()
            .map(OutputPlacement::from)
            .collect())
    }
}

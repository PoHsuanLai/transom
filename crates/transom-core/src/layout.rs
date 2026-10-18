//! Where the outputs sit and where each one lands on a screenshot.
//!
//! The compositor places outputs in one logical space; an output shows `mode` pixels at `scale`
//! pixels per logical unit. A picture of all of them uses the finest scale among them, so no
//! screen loses detail: each output covers its logical rectangle times that scale, and a coarser
//! one is enlarged to fit.

/// A point, in logical units or canvas pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    /// Distance from the left.
    pub x: i32,
    /// Distance from the top.
    pub y: i32,
}

/// A width and a height.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Extent {
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

/// Whether an output is shown turned by a quarter, which swaps its sides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Turn {
    /// Shown the way its mode is.
    Upright,
    /// Shown turned by a quarter (the sides swap).
    Sideways,
}

impl Turn {
    /// The turn a `wl_output` transform value stands for: the values 1, 3, 5 and 7 are quarter
    /// turns (with or without a flip).
    pub fn from_wire(transform: u32) -> Self {
        match transform & 1 {
            1 => Self::Sideways,
            _ => Self::Upright,
        }
    }
}

/// What the compositor says about one output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputFacts {
    /// The connector name, as a capture opens it.
    pub name: String,
    /// Its top-left in the compositor's logical space.
    pub position: Point,
    /// The mode it scans out, in pixels, before any turn.
    pub mode: Extent,
    /// Whether it is shown turned.
    pub turn: Turn,
    /// Pixels per logical unit, as a whole number (0 is taken as 1).
    pub scale: u32,
}

impl OutputFacts {
    /// Its size in logical units: the mode, turned as shown, over the scale (rounded to nearest).
    pub fn logical_size(&self) -> Extent {
        let (width, height) = match self.turn {
            Turn::Upright => (self.mode.width, self.mode.height),
            Turn::Sideways => (self.mode.height, self.mode.width),
        };
        Extent {
            width: divide(width, self.scale),
            height: divide(height, self.scale),
        }
    }
}

/// `value / by` rounded to nearest, with a divisor of 0 taken as 1.
fn divide(value: u32, by: u32) -> u32 {
    let by = u64::from(by.max(1));
    u32::try_from((u64::from(value) + by / 2) / by).unwrap_or(u32::MAX)
}

/// Where one output lands on the canvas, in canvas pixels. The origin is not shifted to zero:
/// an output left of or above the main one has negative coordinates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tile {
    /// The output's name.
    pub name: String,
    /// Its top-left on the canvas.
    pub origin: Point,
    /// The size it covers.
    pub size: Extent,
}

/// The tiles of a picture of all `outputs`, top to bottom then left to right.
///
/// The canvas scale is the finest scale among the outputs. An output with no mode yet is left
/// out, and so is one that shows exactly what an earlier one shows (a mirror), so its pixels are
/// not captured twice.
pub fn arrange(outputs: &[OutputFacts]) -> Vec<Tile> {
    let shown: Vec<&OutputFacts> = outputs
        .iter()
        .filter(|o| {
            let size = o.logical_size();
            size.width > 0 && size.height > 0
        })
        .collect();
    let canvas_scale = shown.iter().map(|o| o.scale.max(1)).max().unwrap_or(1);
    let factor = i32::try_from(canvas_scale).unwrap_or(i32::MAX);
    let mut tiles: Vec<Tile> = shown
        .iter()
        .map(|o| {
            let size = o.logical_size();
            Tile {
                name: o.name.clone(),
                origin: Point {
                    x: o.position.x.saturating_mul(factor),
                    y: o.position.y.saturating_mul(factor),
                },
                size: Extent {
                    width: size.width.saturating_mul(canvas_scale),
                    height: size.height.saturating_mul(canvas_scale),
                },
            }
        })
        .collect();
    tiles.sort_by(|a, b| (a.origin.y, a.origin.x, &a.name).cmp(&(b.origin.y, b.origin.x, &b.name)));
    let mut kept: Vec<Tile> = Vec::with_capacity(tiles.len());
    for tile in tiles {
        if !kept
            .iter()
            .any(|k| k.origin == tile.origin && k.size == tile.size)
        {
            kept.push(tile);
        }
    }
    kept
}

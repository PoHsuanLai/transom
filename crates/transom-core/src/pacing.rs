//! When a held picture goes out again.
//!
//! A screen that does not change sends the stream nothing, yet an app reading the stream expects
//! pictures to keep coming. The stream therefore keeps the newest picture and sends it again to
//! fill the gap. A picture that has just arrived goes out at once; only a picture that was
//! already sent waits for its turn. These functions decide; the stream that does the sending
//! lives in `transom`.

const NANOS_PER_SECOND: u64 = 1_000_000_000;

/// The longest gap between two pictures: a held picture is sent at least once a second.
pub const LONGEST_GAP_NS: u64 = NANOS_PER_SECOND;

/// The shortest gap a repeat is spaced by: a held picture is sent again at most 60 times a second.
pub const SHORTEST_GAP_NS: u64 = NANOS_PER_SECOND / 60;

/// The gap used when the app accepts any rate and names no ceiling: five pictures a second.
pub const OPEN_RATE_GAP_NS: u64 = NANOS_PER_SECOND / 5;

/// A frame rate as PipeWire writes it: pictures per second as a fraction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameRate {
    /// Pictures.
    pub num: u32,
    /// Per this many seconds.
    pub denom: u32,
}

impl FrameRate {
    /// The time between two pictures at this rate, in nanoseconds; `None` for a rate of zero or
    /// one with no divisor.
    fn gap_ns(self) -> Option<u64> {
        match (self.num, self.denom) {
            (0, _) | (_, 0) => None,
            (num, denom) => Some(u64::from(denom) * NANOS_PER_SECOND / u64::from(num)),
        }
    }
}

/// The rate an app and the stream settled on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Negotiated {
    /// One rate.
    Fixed(FrameRate),
    /// Any rate up to a ceiling the app named, or none.
    Open {
        /// The most pictures a second the app wants, if it said.
        ceiling: Option<FrameRate>,
    },
}

impl Negotiated {
    /// The rate for the `framerate` and `max_framerate` of a negotiated format. A zero
    /// `framerate` is PipeWire's way to say the rate is open.
    pub fn from_wire(framerate: FrameRate, max_framerate: FrameRate) -> Self {
        match framerate.gap_ns() {
            Some(_) => Self::Fixed(framerate),
            None => Self::Open {
                ceiling: max_framerate.gap_ns().map(|_| max_framerate),
            },
        }
    }

    /// How long to wait before sending a picture again, in nanoseconds: the gap of a fixed rate;
    /// for an open rate the gap of its ceiling but never shorter than [`OPEN_RATE_GAP_NS`]. Always
    /// within [`SHORTEST_GAP_NS`] and [`LONGEST_GAP_NS`].
    pub fn repeat_gap_ns(self) -> u64 {
        let gap = match self {
            Self::Fixed(rate) => rate.gap_ns().unwrap_or(LONGEST_GAP_NS),
            Self::Open { ceiling } => ceiling
                .and_then(FrameRate::gap_ns)
                .map_or(OPEN_RATE_GAP_NS, |gap| gap.max(OPEN_RATE_GAP_NS)),
        };
        gap.clamp(SHORTEST_GAP_NS, LONGEST_GAP_NS)
    }
}

/// How the newest picture stands with the stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Held {
    /// There is no picture yet.
    Nothing,
    /// It has not been sent.
    Unsent,
    /// It was sent this long ago, in nanoseconds.
    Sent {
        /// Time since it went out.
        ago_ns: u64,
    },
}

/// What to do with a free buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Step {
    /// Send the picture, which is new.
    SendNew,
    /// Send the picture again to fill a gap.
    SendAgain,
    /// Send nothing.
    Hold,
}

/// Decide what to send given how the newest picture stands and the gap between repeats.
///
/// A new picture is never made to wait, so the pace of a changing screen is the screen's own. A
/// repeat goes out only once a full gap has passed since the last send.
pub fn decide(held: Held, gap_ns: u64) -> Step {
    match held {
        Held::Unsent => Step::SendNew,
        Held::Sent { ago_ns } if ago_ns >= gap_ns => Step::SendAgain,
        Held::Nothing | Held::Sent { .. } => Step::Hold,
    }
}

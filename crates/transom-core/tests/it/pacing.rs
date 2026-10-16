use transom_core::pacing::{LONGEST_GAP_NS, OPEN_RATE_GAP_NS, SHORTEST_GAP_NS};
use transom_core::{FrameRate, Held, Negotiated, Step, decide};

const fn rate(num: u32, denom: u32) -> FrameRate {
    FrameRate { num, denom }
}

const MS: u64 = 1_000_000;

#[test]
fn the_gap_follows_the_negotiated_rate_within_the_limits() {
    // (framerate, max_framerate, gap in ns)
    let table = [
        // A fixed rate is its own gap.
        (rate(30, 1), rate(0, 1), 1_000_000_000 / 30),
        (rate(25, 1), rate(60, 1), 40 * MS),
        (rate(60, 1), rate(0, 1), 1_000_000_000 / 60),
        // Too fast a rate is spaced at 60 a second, too slow at one a second.
        (rate(240, 1), rate(0, 1), SHORTEST_GAP_NS),
        (rate(1, 5), rate(0, 1), LONGEST_GAP_NS),
        (rate(2, 1), rate(0, 1), 500 * MS),
        // An open rate with no ceiling, or a ceiling above five a second, fills at five a second.
        (rate(0, 1), rate(0, 1), OPEN_RATE_GAP_NS),
        (rate(0, 1), rate(60, 1), OPEN_RATE_GAP_NS),
        (rate(0, 1), rate(5, 1), OPEN_RATE_GAP_NS),
        // A ceiling below that is honoured, but never slower than once a second.
        (rate(0, 1), rate(2, 1), 500 * MS),
        (rate(0, 1), rate(1, 10), LONGEST_GAP_NS),
        // A rate with no divisor is no rate.
        (rate(30, 0), rate(0, 0), OPEN_RATE_GAP_NS),
    ];
    for (framerate, max, expected) in table {
        assert_eq!(
            Negotiated::from_wire(framerate, max).repeat_gap_ns(),
            expected,
            "{framerate:?} {max:?}"
        );
    }
}

#[test]
fn a_picture_is_sent_when_new_and_repeated_only_after_a_gap() {
    let gap = 100 * MS;
    // (how the picture stands, what to do)
    let table = [
        (Held::Nothing, Step::Hold),
        (Held::Unsent, Step::SendNew),
        (Held::Sent { ago_ns: 0 }, Step::Hold),
        (Held::Sent { ago_ns: gap - 1 }, Step::Hold),
        (Held::Sent { ago_ns: gap }, Step::SendAgain),
        (Held::Sent { ago_ns: 10 * gap }, Step::SendAgain),
    ];
    for (held, expected) in table {
        assert_eq!(decide(held, gap), expected, "{held:?}");
    }
}

#[test]
fn a_new_picture_never_waits_for_the_gap() {
    // Even right after a send, a picture that has not gone out is sent at once.
    for gap in [SHORTEST_GAP_NS, OPEN_RATE_GAP_NS, LONGEST_GAP_NS] {
        assert_eq!(decide(Held::Unsent, gap), Step::SendNew);
    }
}

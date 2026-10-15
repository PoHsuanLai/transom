use transom_core::{Extent, OutputFacts, Point, Tile, Turn, arrange};

fn output(name: &str, at: (i32, i32), mode: (u32, u32), turn: Turn, scale: u32) -> OutputFacts {
    OutputFacts {
        name: name.into(),
        position: Point { x: at.0, y: at.1 },
        mode: Extent {
            width: mode.0,
            height: mode.1,
        },
        turn,
        scale,
    }
}

fn tile(name: &str, at: (i32, i32), size: (u32, u32)) -> Tile {
    Tile {
        name: name.into(),
        origin: Point { x: at.0, y: at.1 },
        size: Extent {
            width: size.0,
            height: size.1,
        },
    }
}

#[test]
fn a_picture_of_the_outputs_is_laid_out_at_the_finest_scale() {
    use Turn::{Sideways, Upright};
    // (what the compositor says, the tiles of the picture)
    let table = [
        (
            "one screen",
            vec![output("A", (0, 0), (1920, 1080), Upright, 1)],
            vec![tile("A", (0, 0), (1920, 1080))],
        ),
        (
            "one sharp screen is its own pixels",
            vec![output("A", (0, 0), (3840, 2160), Upright, 2)],
            vec![tile("A", (0, 0), (3840, 2160))],
        ),
        (
            "a coarse screen beside a sharp one is enlarged",
            vec![
                output("A", (0, 0), (1920, 1080), Upright, 1),
                output("B", (1920, 0), (3840, 2160), Upright, 2),
            ],
            vec![
                tile("A", (0, 0), (3840, 2160)),
                tile("B", (3840, 0), (3840, 2160)),
            ],
        ),
        (
            "a screen left of and above the main one has negative places, listed top first",
            vec![
                output("B", (0, 0), (1920, 1080), Upright, 1),
                output("A", (-1280, -720), (1280, 720), Upright, 1),
            ],
            vec![
                tile("A", (-1280, -720), (1280, 720)),
                tile("B", (0, 0), (1920, 1080)),
            ],
        ),
        (
            "a screen turned a quarter swaps its sides",
            vec![output("A", (0, 0), (1080, 1920), Sideways, 1)],
            vec![tile("A", (0, 0), (1920, 1080))],
        ),
        (
            "a mirror is captured once",
            vec![
                output("B", (0, 0), (1920, 1080), Upright, 1),
                output("A", (0, 0), (1920, 1080), Upright, 1),
            ],
            vec![tile("A", (0, 0), (1920, 1080))],
        ),
        (
            "an output with no mode yet is left out; scale 0 is 1",
            vec![
                output("A", (0, 0), (0, 0), Upright, 1),
                output("B", (0, 0), (800, 600), Upright, 0),
            ],
            vec![tile("B", (0, 0), (800, 600))],
        ),
        ("no outputs", vec![], vec![]),
    ];
    for (what, outputs, want) in table {
        assert_eq!(arrange(&outputs), want, "{what}");
    }
}

#[test]
fn quarter_turns_are_the_odd_transform_values() {
    let sideways: Vec<u32> = (0..8)
        .filter(|&t| Turn::from_wire(t) == Turn::Sideways)
        .collect();
    assert_eq!(sideways, vec![1, 3, 5, 7]);
}

//! Listing the outputs: the name, place, mode, turn and scale each `wl_output` reports.

use transom_core::{Extent, OutputFacts, Point, Turn};
use wayland_client::protocol::wl_output;
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum};

use super::{CaptureError, Link, OutputId, Wl, missing};

/// What one `wl_output` has said so far.
#[derive(Debug, Default)]
pub(super) struct SeenOutput {
    /// Its connector name (an output that never says one cannot be opened and is not listed).
    pub name: Option<String>,
    position: (i32, i32),
    /// The current mode, in pixels.
    mode: (u32, u32),
    transform: u32,
    scale: u32,
}

impl SeenOutput {
    /// The output as the layout sees it, once it has said a name.
    fn facts(&self) -> Option<OutputFacts> {
        Some(OutputFacts {
            name: self.name.clone()?,
            position: Point {
                x: self.position.0,
                y: self.position.1,
            },
            mode: Extent {
                width: self.mode.0,
                height: self.mode.1,
            },
            turn: Turn::from_wire(self.transform),
            scale: self.scale,
        })
    }
}

impl Link {
    /// Bind every `wl_output` the compositor advertises, each with the name of its global.
    pub(super) fn bind_outputs(&self) -> Result<Vec<(u32, wl_output::WlOutput)>, CaptureError> {
        let advertised: Vec<u32> = self.wire.globals.contents().with_list(|list| {
            list.iter()
                .filter(|g| g.interface == "wl_output")
                .map(|g| g.name)
                .collect()
        });
        advertised
            .into_iter()
            .map(|global| {
                self.wire
                    .globals
                    .bind::<wl_output::WlOutput, _, _>(&self.qh, 1..=4, OutputId(global))
                    .map(|output| (global, output))
                    .map_err(missing)
            })
            .collect()
    }

    /// Every output the compositor advertises now, with its place, mode and scale. The position
    /// is the output's top-left in the compositor's logical space and the scale is the whole
    /// number `wl_output` reports (a fractional scale is rounded up there).
    pub fn output_facts(mut self) -> Result<Vec<OutputFacts>, CaptureError> {
        let bound = self.bind_outputs()?;
        self.roundtrip()?;
        self.roundtrip()?;
        let facts = self
            .wl
            .outputs
            .values()
            .filter_map(SeenOutput::facts)
            .collect();
        release_all(bound.into_iter().map(|(_, output)| output));
        Ok(facts)
    }
}

/// Let the compositor drop outputs that were bound only to be read, since the connection outlives
/// the step. One too old to be released (before version 3) stays until the connection closes.
pub(super) fn release_all(outputs: impl IntoIterator<Item = wl_output::WlOutput>) {
    for output in outputs.into_iter().filter(|o| o.version() >= 3) {
        output.release();
    }
}

impl Dispatch<wl_output::WlOutput, OutputId> for Wl {
    fn event(
        state: &mut Self,
        _: &wl_output::WlOutput,
        event: wl_output::Event,
        id: &OutputId,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let seen = state.outputs.entry(id.0).or_default();
        match event {
            wl_output::Event::Name { name } => seen.name = Some(name),
            wl_output::Event::Geometry {
                x, y, transform, ..
            } => {
                seen.position = (x, y);
                seen.transform = match transform {
                    WEnum::Value(turn) => u32::from(turn),
                    WEnum::Unknown(value) => value,
                };
            }
            wl_output::Event::Mode {
                flags,
                width,
                height,
                ..
            } => {
                let current = match flags {
                    WEnum::Value(flags) => flags.contains(wl_output::Mode::Current),
                    WEnum::Unknown(_) => false,
                };
                if current {
                    seen.mode = (
                        u32::try_from(width).unwrap_or(0),
                        u32::try_from(height).unwrap_or(0),
                    );
                }
            }
            wl_output::Event::Scale { factor } => {
                seen.scale = u32::try_from(factor).unwrap_or(1);
            }
            _ => {}
        }
    }
}

# transom

The desktop portal backend for the quire desktop: screenshots, screen sharing (PipeWire, outputs
and single windows, restore tokens), remote desktop, settings and the "keep the screen on" inhibit,
served to apps through `xdg-desktop-portal` as the `quire` portal implementation.

It runs as its own process and talks to the compositor (casement) through the standard Wayland
image-capture protocols, so it works with any compositor that offers them. The file chooser stays
with `xdg-desktop-portal-gtk`.

Status: being built (phase P2 of casement). Licensed under either of MIT or Apache-2.0, at your
option.

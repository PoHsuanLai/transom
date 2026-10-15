# Architecture

transom is the quire desktop's portal backend. `xdg-desktop-portal` (a separate process) asks it
over the session bus; it answers with the `org.freedesktop.impl.portal.*` interfaces. It serves
Screenshot, ScreenCast, RemoteDesktop, Settings and Inhibit. The file chooser stays with the GTK
backend (see `dist/quire-portals.conf`).

## 1. Crates and allowed edges

| Crate | Purpose | I/O |
| --- | --- | --- |
| `transom-core` | the screen-cast session machine (`step`), restore tokens and book, the source choice, the inhibit book, the settings values and their wire numbers | none |
| `transom` | the five D-Bus interfaces (zbus), the seams and their real and fake implementations, `serve`, the binary | D-Bus, Wayland, PipeWire, files |

`transom-core` depends on `thiserror` only. It reaches no zbus, tokio, pipewire, wayland, image
or tracing (row T1 of `scripts/check-boundary.sh`) and reads no clock, environment or file (T5).

Inside `transom` only these modules name the heavy crates: `capture/` the Wayland crates (T2),
`screencast/pipewire.rs` the PipeWire crate (T3), `screenshot.rs` the image encoder (T4); only
`main.rs` reads the environment (T6).

## 2. Layout

```
crates/transom-core/src/{lib, screencast, restore, choice, inhibit, settings_values}.rs
crates/transom/src/{lib, main, error, chooser, screenshot, remote_desktop, settings, inhibit}.rs
                   screencast/{mod, session, stream, pipewire, restore}.rs
                   capture/{mod, fake, output, toplevel}.rs
dist/   quire.portal  quire-portals.conf  org.freedesktop.impl.portal.desktop.quire.service  transom.service
scripts/install.sh   installs the binary, the unit, the D-Bus file and the portal file (standalone; --prefix, --destdir, --bin-dir)
dev/    up.sh down.sh lib/{jail,chain}.sh accept/*.sh        (the jailed portal chain)
scripts/check-boundary.sh
```

## 3. The seams

| Trait | Real | Fake | Fill lane |
| --- | --- | --- | --- |
| `capture::CaptureSource` | `ExtCapture` (`OutputCapture`, `ToplevelCapture`) | `FakeCapture` | P2-T2 |
| `screencast::stream::FrameSink` | `PipewireSink` | `FakeSink` | P2-T2 |
| `screencast::restore::RestoreStore` | `FileStore` | `MemoryStore` | P2-T2 |
| `chooser::Chooser` | `ShellChooser` (calls the shell's picker) | `FakeChooser` | P2-T2 (chooser client) |
| `settings::AppearanceSource` | `DesktopAppearance` | `FixedAppearance` | P2-T1 |
| `remote_desktop::Consent` | `ShellConsent` (the chooser path) | `FakeConsent` | P2-T3 |
| `remote_desktop::EisHandout` | `CasementEis` (`org.quire.Compositor1.ConnectEis`) | `FakeEis` | P2-T3 |

`Pump` (`screencast/stream.rs`) is the loop body between a `CaptureSource` and a `FrameSink`, and
is implemented and tested against the fakes.

## 4. Flow of a screen share

1. `CreateSession` opens a `SessionBook` entry.
2. `SelectSources` records the app's request (`Selection`).
3. `Start` feeds `CastIn::Start { restored }`, where `restored` is what the app's restore token
   resolves to now (`RestoreBook::resolve`). The machine answers `AskChooser` or `OpenStream`.
4. The chooser's answer is checked by `ChoiceRequest::accept` and fed back as `CastIn::Picked`.
5. `OpenStream` builds an `ExtCapture` and a `PipewireSink` and runs a `Pump` on its own thread;
   `NodeReady` returns the node and the reply carries it.
6. A closed window or unplugged display ends the capture (`PumpOutcome::SourceEnded`,
   `CastIn::StreamEnded`).

## 4b. Flow of remote control

`CreateSession` opens a session (and serves the session object at its handle); `SelectDevices`
records `types`; `Start` asks the person through `Consent` and answers with the devices granted;
`ConnectToEIS` (only after a yes) asks the compositor for a socket with `EisHandout`, which
tags everything the app sends as remote input of this session. The `Notify*` methods are
accepted and dropped: input comes only over the socket.

## 5. Window capture

Single windows come from the compositor's `ext_foreign_toplevel_image_capture_source_manager_v1`
with `ext_image_copy_capture_manager_v1`. The compositor sizes frames at logical size times scale
and sends one only when the window changed; transom does not poll.

## 6. The jailed chain

`dev/up.sh` starts a private bus, PipeWire, WirePlumber, transom and `xdg-desktop-portal` inside
the desktop harness's jail, with `dist/quire.portal` found through `XDG_DESKTOP_PORTAL_DIR`.
`dev/accept/*.sh` drive it. Unit tests never need it.

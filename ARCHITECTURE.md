# Architecture

transom is the quire desktop's portal backend. `xdg-desktop-portal` (a separate process) asks it
over the session bus; it answers with the `org.freedesktop.impl.portal.*` interfaces. It serves
Screenshot, ScreenCast, RemoteDesktop, Settings, Inhibit and Access. The file chooser stays with the GTK
backend (see `dist/quire-portals.conf`).

## 1. Crates and allowed edges

| Crate | Purpose | I/O |
| --- | --- | --- |
| `transom-core` | the screen-cast session machine (`step`), restore tokens and book, the source choice, the inhibit book, the layout of a picture of all outputs, when a held picture is sent again (`pacing`), the settings values and their wire numbers | none |
| `transom` | the six D-Bus interfaces (zbus), the seams and their real and fake implementations, `serve`, the binary | D-Bus, Wayland, PipeWire, files |

`transom-core` depends on `thiserror` only. It reaches no zbus, tokio, pipewire, wayland, image
or tracing (row T1 of `scripts/check-boundary.sh`) and reads no clock, environment or file (T5).

Inside `transom` only these modules name the heavy crates: `capture/` the Wayland crates (T2),
`screencast/pipewire.rs` the PipeWire crate (T3), `screenshot.rs` the image encoder (T4); only
`main.rs` reads the environment (T6).

## 2. Layout

```
crates/transom-core/src/{lib, screencast, restore, choice, inhibit, layout, pacing, settings_values}.rs
crates/transom/src/{lib, main, error, link, peers, names, fsread, random, chooser, screenshot, settings, access}.rs
                   screenshot/{layout, color, service}.rs
                   remote_desktop/{mod, book, sessions, consent, eis, devices}.rs
                   inhibit/{mod, hub, effect}.rs
                   screencast/{mod, flow, options, run, session, stream, pipewire, pixels, restore}.rs
                   capture/{mod, fake, output, toplevel, wl}.rs  capture/wl/{outputs, wait}.rs
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
| `screenshot::OutputLayout` | `LiveLayout` (asks the compositor at each screenshot) | `FixedLayout` | transom-shot |
| `screenshot::ColorPicker` | `ShellColorPicker` (`org.quire.Shell1.PickColor`) | `FakeColorPicker` | transom-shot |
| `chooser::Chooser` | `ShellChooser` (calls the shell's picker) | `FakeChooser` | P2-T2 (chooser client) |
| `link::BusLink` | the connection that owns the portal's name, attached by `serve` | (none: tests use `serve_on` on a private bus) | PORTAL-01 |
| `settings::AppearanceSource` | `DesktopAppearance` | `FixedAppearance` | P2-T1 |
| `access::Asker` | `ShellAsker` (`org.quire.Shell1.AskPermission`) | `FakeAsker` | transom-access |
| `remote_desktop::Consent` | `ShellConsent` (its own `AskPermission` question naming the input) | `FakeConsent` | P2-T3 |
| `remote_desktop::EisHandout` | `CasementEis` (`org.quire.Compositor1.ConnectEis` and `EndEis`) | `FakeEis` | P2-T3 |

`Pump` (`screencast/stream.rs`) is the loop body between a `CaptureSource` and a `FrameSink`, and
is implemented and tested against the fakes. A turn waits at most `QUIET_TURN` for a frame and reports
`Quiet` if the picture did not change; the sink, not the pump, keeps the newest picture and sends it
again at the stream's negotiated pace (`transom_core::decide`), so a still screen still gives a stream.

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

## 3b. Who asks, and from where

Every question to the shell or the compositor goes out on the one connection that owns
`org.freedesktop.impl.portal.desktop.quire` (`link::BusLink`, attached by `serve` before the name is
claimed). The shell answers only the owner of that name, and a second connection has another
unique name. `tests/it/shell_bus.rs` pairs the real clients with a stand-in shell that applies the
shell's owner check, on a private bus.

## 3c. Lifetimes and limits

- A screen-cast session, a remote-control session and an Inhibit hold each hang on a `peers::Tether`:
  the unique name of whoever made the call and of the portal frontend at that moment. When either
  leaves the bus, or the frontend gives up `org.freedesktop.portal.Desktop`, `peers::spawn_watch`
  ends everything tied to it (streams stop, sockets are taken back, holds are released).
- Counts are capped per caller and in total (`peers::SESSION_LIMITS`, `HOLD_LIMITS`, `MAX_MONITORS`).
  A full book drops sessions that never began and are older than `UNSTARTED_TTL` before it refuses.
- Every wait on the compositor has a deadline (`capture/wl.rs`: connect, round trip, buffer
  constraints, first picture), as have the logind and `ConnectEis`/`EndEis` calls. Waits on the person
  (the picker, the sheets) have none. One screenshot is taken at a time, within `SHOT_DEADLINE`.
- A captured picture is at most 16384 pixels on an edge and 256 MiB; a screenshot canvas at most
  512 MiB. Frame memory is an anonymous memfd, not a file in `/dev/shm`.
- Files that someone else could replace (the appearance document, the restore file, a desktop entry)
  are read with `fsread::read_text`: no FIFO, no device, a size limit, never on the async runtime.

## 4b. Flow of remote control

`CreateSession` opens a session (and serves the session object at its handle); `SelectDevices`
records `types`; `Start` asks the person through `Consent`, a question of its own that names the
input the app would take over ("keyboard and pointer"), and answers with the devices granted; a
screen-sharing pick is never a yes. The yes is bound to the session it was asked about. `ConnectToEIS`
(only after a yes, once per session) asks the compositor for a socket with `EisHandout`, which tags
everything the app sends as remote input of this session; closing the session calls `EndEis`, so the
compositor stops that input and closes its end. The `Notify*` methods are accepted and dropped: input
comes only over the socket.

## 4c. Screenshot consent

The frontend asks the person (its Access step) for a plain request and says so with
`permission_store_checked`. It skips that step for an `interactive` request, whose dialog is the
backend's, and sets the same flag anyway. So the backend asks (`AskPermission`) for every
`interactive` request and for every request without the flag; it stays quiet only for a
non-interactive request the frontend vouched for. With no shell to ask, the request is cancelled.

## 4d. Restore tokens

A grant carries its persist mode and the time it was given. "While running" grants last a day,
"until revoked" ones thirty days (the interval at which macOS asks again); after that the person is
asked once more. An app with an empty app id (not sandboxed, no identity) is never remembered:
every app without an identity would share one grant, so it is asked every time. The book holds at
most 256 grants (the oldest go first). A damaged file is moved to `<name>.bad`, a flawed line is
skipped and a copy of the file is kept beside it.

## 5. Window capture

Single windows come from the compositor's `ext_foreign_toplevel_image_capture_source_manager_v1`
with `ext_image_copy_capture_manager_v1`. The compositor sizes frames at logical size times scale
and sends one only when the window changed; transom does not poll.

## 6. The jailed chain

`dev/up.sh` starts a private bus, PipeWire, WirePlumber, transom and `xdg-desktop-portal` inside
the desktop harness's jail, with `dist/quire.portal` found through `XDG_DESKTOP_PORTAL_DIR`.
`dev/accept/*.sh` drive it. Unit tests never need it.

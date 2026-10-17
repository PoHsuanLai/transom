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
crates/transom/src/{lib, main, error, link, peers, dev_rows (feature debug), shell, names, fsread, random, chooser, screenshot, settings, access}.rs
                   screenshot/{layout, color, service}.rs
                   remote_desktop/{mod, book, sessions, object, consent, eis, devices}.rs
                   inhibit/{mod, hub, effect}.rs
                   screencast/{mod, flow, options, run, session, stream, pipewire, pixels, restore}.rs
                   capture/{mod, display, fake, job, line, output, toplevel, wl}.rs  capture/line/{casement, fake}.rs  capture/wl/{outputs, wait, wire}.rs
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
| `link::BusLink` | the connection that owns the portal's name and the `peers::Peers` that tell callers apart, both attached by `serve` | (none: tests use `serve_on` on a private bus and `Peers::introduce`) | PORTAL-01, 03, 10 |
| `settings::AppearanceSource` | `DesktopAppearance` | `FixedAppearance` | P2-T1 |
| `access::Asker` | `ShellAsker` (`org.quire.Shell1.AskPermission`) | `FakeAsker` | transom-access |
| `remote_desktop::Consent` | `ShellConsent` (its own `AskPermission` question naming the input) | `FakeConsent` | P2-T3 |
| `remote_desktop::EisHandout` | `CasementEis` (`org.quire.Compositor1.ConnectEis` and `EndEis`) | `FakeEis` | P2-T3 |
| `capture::PortalLine` | `CasementLine` (`org.quire.Compositor1.Connect("portal")`: a Wayland connection of the portal's own, trusted for the screen and the windows; casement limits how often a caller may ask, so a capture job, `capture::Job`, asks once and runs every step on that connection) | `FakeLine` | NEW-FA-2 |

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
5. `OpenStream` builds an `ExtCapture` and a `PipewireSink` for each source and runs a `Pump` on its own thread;
   `NodeReady` returns the node and the reply carries it. The Start is one `capture::Job`: the restore
   lookup and every stream use the one connection to the compositor, asked for once.
6. A closed window or unplugged display ends the capture (`PumpOutcome::SourceEnded`,
   `CastIn::StreamEnded`).

## 3b. Who asks, and from where

Every question to the shell or the compositor goes out on the one connection that owns
`org.freedesktop.impl.portal.desktop.quire` (`link::BusLink`, attached by `serve` before the name is
claimed). The shell answers only the owner of that name, and a second connection has another
unique name. `tests/it/shell_bus.rs` pairs the real clients with a stand-in shell that applies the
shell's owner check, on a private bus.

## 3b-2. Who may call, and whom the portal asks (PORTAL-03, PORTAL-10)

Caller identity is `peephole-bus` (git, tag v0.1.4, feature `tokio` and `desktop`); `serve` builds one
`peers::Peers` on the name-owning connection from the root-owned caller table
(`/etc/quire/desktop-callers.toml`, loaded by `main.rs`; a table that cannot be loaded is empty, so nobody is
recognised, every call is refused and the shell is never asked).

- **Inbound.** Every method of every `org.freedesktop.impl.portal.*` interface (Screenshot, ScreenCast,
  RemoteDesktop with its `Notify*` and `ConnectToEIS`, Access, Inhibit, Settings, and the `Session` and `Request`
  objects) starts with `peers::FrontendOnly::admit`: the sender must hold role `portal_frontend`, proved `Full`
  (image and unit as the unit manager states it). The refusal is `AccessDenied` (Settings: the portal's
  `NotAllowed`). Before `serve` has attached the means of telling callers apart, nobody passes. Properties
  (`version`, `AvailableSourceTypes`, ...) are answered by zbus without a guard; they carry no state.
- **Creator.** A session or request remembers the sender that opened it (`peers::Creator`: the `Tether`'s caller for
  screen-cast and remote-control sessions, a field of the object for Inhibit requests and monitors). Every later
  call about it (`Select*`, `Start`, `ConnectToEIS`, `Notify*`, `QueryEndResponse`, `Close`) must come from that
  same peer (`FrontendOnly::admit_creator`). A call about a session that is not open gets only the frontend check
  and then the old answer (response 2), so a refusal says nothing about which handles exist.
- **Outbound.** `shell::Door::open` runs once per question: subscribe to changes of the owner of `org.quire.Shell`,
  require that the owner holds role `shell`, proved `Full`, and address the question to that owner's unique name.
  Nothing is cached, so a shell restart is seen by the next question. `shell::answered` ends a question that is open
  when the name changes hands (`ChooserError::ShellChanged`). A name nobody owns, an owner that is not the shell and a
  bus that does not answer are `ChooserError::NotTheShell`. `ChooserError::is_refusal` groups these with
  `Unavailable`: the picker is a cancel, a sheet is a no, remote control grants nothing. Picker, sheets and colour
  picker all go through `Door`.
- **The person's own rows (feature `debug`).** The desktop's test jail has no `/etc/quire/desktop-callers.toml` and no
  unit manager, so a stand-in frontend or shell built in a checkout could never be recognised. Built with
  `--features debug` (which alone turns on `peephole-bus/dev-rows`; it is no default feature and no shipped build has
  it), `main.rs` reads `TRANSOM_DEV_CALLERS=FILE`, the only place it is read, and `dev_rows::with_dev_rows` adds the
  file's `[[peer]]` rows after the shipped table (a plain file of the person's, or root's, that group and others cannot
  write; a link is refused). A file that cannot be taken stops the start with the reason in the log. A caller a row names
  has `Proof::Dev`, below every real proof; `peers::need` lists `Proof::Dev` beside `Proof::Full` for the frontend and the
  shell in that build only (which also lets in the real program at any proof). A release build has no variable, no
  module and no `dev-rows`. Limits: a debug build is not a security boundary (the person's own file says who is a
  service), and a row never outranks a shipped row for the same image or unit. The jail's scripts pass
  `TRANSOM_DEV_CALLERS` on (`dev/lib/chain.sh`).
- Tests: `tests/it/guard.rs` (each method refuses a stranger, the shell and a frontend proved only by image; sessions
  and monitors are closed and changed only by their opener) and `tests/it/shell_check.rs` (a squatter is never asked,
  the owner is looked up again after a restart, a question open when the shell changes ends as a no), on a private
  bus with `Peers::introduce`; nothing reads `/proc`.

## 3c. Lifetimes and limits

- A screen-cast session, a remote-control session and an Inhibit hold each hang on a `peers::Tether`:
  the unique name of whoever made the call and of the portal frontend at that moment. When either
  leaves the bus, or the frontend gives up `org.freedesktop.portal.Desktop`, `peers::spawn_watch`
  ends everything tied to it (streams stop, sockets are taken back, holds are released).
- Counts are capped per caller and in total (`peers::SESSION_LIMITS`, `HOLD_LIMITS`, `MAX_MONITORS`).
  A full book drops sessions that never began and are older than `UNSTARTED_TTL` before it refuses.
- Every wait on the compositor has a deadline (`capture/wl.rs`, `wl/wire.rs`: connect, round trip, buffer
  constraints, first picture), as have the logind, `ConnectEis`/`EndEis` and `Connect("portal")` calls. Waits on the person
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
comes only over the socket. A restart of the portal loses its book while the compositor keeps what it handed out,
so session numbers count up from the clock (never reused) and a start asks the compositor to drop the earlier
run's sockets (`EndAllEis`; see FINDINGS NEW-PI-8).

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

A grant is made, and a token honoured, only for an app that is shown to be what its id says
(`app_proof.rs`, NEW-PI-2). The id is the frontend's word: for a sandboxed app read from the sandbox, for a host
app the app's own claim (it registers it, or names its launcher scope). So the app's bus connection is read from
its session handle (the frontend builds `.../session/<sender>/<token>` from the real sender, and the frontend is
proved `Full`), `peers.who` says who is on it, and:

- a sandboxed app (the sandbox's metadata agrees with its control group) whose sandbox names the id is remembered;
- a host program whose image is root-owned and is the program the id's desktop entry runs, the entry being
  root's to write (the first entry found decides, so one in the person's home registers nothing), is remembered;
- anything else, a host app that only declares an id included, gets the sheet every time and no token.

`Parts.prover` is `None` in a real run, which `serve` turns into the `PeerProver`; a service built with no prover
(`ScreenCastService::new`) remembers nobody.

### Taking a remembered share back (NEW-PI-7)

`org.quire.Portal1` is served at the portal's path beside the portal interfaces, over the same book as ScreenCast
(`screencast/grants.rs`). `ListGrants() -> a(ssut)` gives one entry per app that has a grant still in force: app id,
the name to show, the persist mode (1 while it runs, 2 until taken back) and the date. `Revoke(app_id: s) -> u` forgets
every grant of the app and says how many went. Neither names a token. Only the Settings app may call, proved `Full`
like the frontend (`peers::SettingsOnly`); the Privacy page of Settings (detent) is the person's way in (see the ask in
FINDINGS). A running share is not stopped by taking back its grant.

## 5. Window capture

Single windows come from the compositor's `ext_foreign_toplevel_image_capture_source_manager_v1`
with `ext_image_copy_capture_manager_v1`. The compositor sizes frames at logical size times scale
and sends one only when the window changed; transom does not poll.

## 6. The jailed chain

`dev/up.sh` starts a private bus, PipeWire, WirePlumber, transom and `xdg-desktop-portal` inside
the desktop harness's jail, with `dist/quire.portal` found through `XDG_DESKTOP_PORTAL_DIR`.
`dev/accept/*.sh` drive it. Unit tests never need it.

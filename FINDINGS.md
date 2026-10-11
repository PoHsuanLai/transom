# Findings

Open items and standing facts. An entry names the condition that closes it.

## Stubs behind frozen interfaces

21 `todo!()` (P2-T3 filled `remote_desktop.rs`; its rows are gone), all in the `transom` crate; `transom-core` has none. The fill lanes are P2-T1 to
P2-T3 and the chooser client (listed in the program's transom fill-lane notes).

| File | Items | Lane | Proven by |
| --- | --- | --- | --- |
| `crates/transom/src/settings.rs` | `DesktopAppearance::current`, `SettingsService::read_all`, `read` (3) | P2-T1 | `tests/it/fill.rs::the_desktop_appearance_reads_without_error`, `dev/accept/settings.sh` |
| `crates/transom/src/screenshot.rs` | `screenshot`, `pick_color` (2) | P2-T1 | `dev/accept/screenshot.sh` |
| `crates/transom/src/inhibit.rs` | `inhibit`, `create_monitor`, `query_end_response` (3) | P2-T1 | `dev/accept/inhibit.sh` |
| `crates/transom/src/screencast/mod.rs` | `create_session`, `select_sources`, `start` (3) | P2-T2 | `dev/accept/screencast-output.sh`, `screencast-window.sh` |
| `crates/transom/src/screencast/pipewire.rs` | `PipewireSink::open`, `push`, `resize` (3) | P2-T2 | `tests/it/fill.rs::the_pipewire_sink_accepts_a_run_of_frames` |
| `crates/transom/src/screencast/restore.rs` | `FileStore::load`, `save` (2) | P2-T2 | `tests/it/fill.rs::the_file_store_round_trips_a_book_and_a_missing_file_is_empty` |
| `crates/transom/src/capture/output.rs` | `OutputCapture::open`, `frame` (2) | P2-T2 | `dev/accept/screencast-output.sh` |
| `crates/transom/src/capture/toplevel.rs` | `ToplevelCapture::open`, `frame` (2) | P2-T2 | `dev/accept/screencast-window.sh` |
| `crates/transom/src/chooser.rs` | `ShellChooser::pick` (1) | P2-T2 (chooser client) | `dev/accept/chooser.sh` |

Ignored tests: P2-T1 1, P2-T2 2 (`crates/transom/tests/it/fill.rs`).

## Open items

1. **The shell side of the chooser does not exist yet.** `org.quire.Shell1.PickCaptureSource` is
   frozen here as the client (`chooser.rs`, the `Shell1` proxy): `(app_label: s, kinds: u,
   multiple: u, cursor_modes: au) -> (sources: a(ss), cursor: u)` on name `org.quire.Shell`, path
   `/org/quire/Shell1`. The shell serves it.
2. **Settings source.** `DesktopAppearance` reads the desktop settings document. Reading it with
   quire's `ds-settings` (v0.3.16, rev `b2dab9ce9b8cb2150b7050b4529d9479f659d3cf`) needs that
   dependency in `crates/transom/Cargo.toml`; P2-T1 adds it there and nowhere else. `transom-core`
   does not need it: the portal's contract is three keys.
3. **Restore data shape.** The frontend passes `restore_data` as `(suv)` (vendor, version, data)
   in current interface versions. P2-T2 checks it against the installed interface XML and uses
   `("quire", 1, <token>)`; `RestoreToken` is the token.
4. **`UseIn=quire`** in `dist/quire.portal` matches `XDG_CURRENT_DESKTOP`; the session must set
   it to `quire`.
5. **PipeWire needs its development files** to build the `pipewire` crate (libpipewire and clang).
6. **Interface XML not re-read.** The method signatures of the five interfaces were written from
   the portal documentation as remembered; the first fill lane diffs them against the installed
   interface files and fixes any difference.

## Standing facts

- The screen-cast session `step` function is total: every (state, input) pair that matters has
  a row in `crates/transom-core/tests/it/screencast.rs`; anything else leaves the session as it is.
- A restore token resolves all or nothing, and only for the app it was given to.
- No accent chosen is published as an out-of-range colour, as the specification says.

## p2-t3: RemoteDesktop (2026-10-11)

Not compiled; the orchestrator gates. `remote_desktop.rs` has no `todo!()`.

- **Shape.** The interface methods wrap public methods of `RemoteDesktopService` (`create`, `select`, `begin`, `connect`,
  `close`) so tests run without a bus. Seams: `Consent` (`ShellConsent`, `FakeConsent`) and `EisHandout` (`CasementEis`,
  `FakeEis`); `Parts` gained `consent` and `eis` (built inside `Parts::real`, whose signature is unchanged).
- **EIS only.** `ConnectToEIS` asks `org.quire.Compositor1.ConnectEis(session: t, devices: u) -> h` (casement; only
  `transom.service` may call it). The `Notify*` methods are accepted and dropped (debug log): there is no second way in.
  Checked against the installed `org.freedesktop.impl.portal.RemoteDesktop.xml`: all signatures match the skeleton; `ConnectToEIS`
  now returns `fdo::Result<h>` so a refusal is an error and not a panic.
- **Session object.** The frontend closes a session through `org.freedesktop.impl.portal.Session.Close` at the session handle; a
  small `SessionObject` is served there (another portal interface serving the same handle is tolerated). No `Closed` signal yet.
- **Open: consent is the sharing picker.** The shell has no "allow remote control" question, so `ShellConsent` asks
  `PickCaptureSource` (any pick is a yes, cancel a no). A dedicated `org.quire.Shell1` method is an interface ask for sill
  (suggested: `AskRemoteControl(app_label: s, devices: u) -> (allowed: u)`). The app's label is its app id until a name lookup
  exists ("An app" when empty); that breaks the plain-words rule for sandboxed ids.
- **Open: combined sessions.** An app that shares a screen and controls it in one session expects `streams` in the `Start`
  reply. `ScreenCastService` keeps its `SessionBook` private, so this service cannot reach it; today `Start` answers `devices`
  only. The orchestrator can close this after P2-T2 with a shared book (or a `streams_for(&SessionKey)` on the screencast
  service). `persist_mode` and `restore_data` are not honoured (no `restore_data` is returned).
- **Open: request objects.** `Start` waits for the person; the frontend cancels it with `Request.Close` on `handle`, which no
  interface here serves yet (same for ScreenCast and Screenshot).
- **Open: acceptance.** `dev/accept/remote-desktop.sh` is still a skeleton: it needs a stub `org.quire.Shell1` on the private
  bus (for consent), the unit-check override in casement's debug peer table (so the jailed transom reads as
  `transom.service`), and a client that receives a file descriptor over D-Bus and speaks EIS. None exists in the jail.
- **Spots most likely not to compile.** `OwnedValue::downcast_ref::<u32>()` on `&OwnedValue`; `#[zbus(object_server)]` as a
  parameter next to `ObjectPath` arguments; `server.at(session_handle, object)` and `server.remove::<SessionObject, _>(&str)`;
  the `From<zbus::zvariant::OwnedFd> for std::os::fd::OwnedFd` conversion; `name.as_str()` on the `MethodError` name;
  `OwnedValue::try_from(Value::from(u32))`.

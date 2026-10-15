# Findings

Open items and standing facts. An entry names the condition that closes it.

## Stubs behind frozen interfaces

No `todo!()` left: P2-T1, P2-T2 (with the chooser client) and P2-T3 are filled.

One ignored test: `fill::the_pipewire_sink_accepts_a_run_of_frames` needs a PipeWire daemon; the jailed portal chain runs it.

## Open items

1. **The shell side of the chooser does not exist yet.** `org.quire.Shell1.PickCaptureSource` is
   frozen here as the client (`chooser.rs`, the `Shell1` proxy): `(app_label: s, kinds: u,
   multiple: u, cursor_modes: au) -> (sources: a(ss), cursor: u)` on name `org.quire.Shell`, path
   `/org/quire/Shell1`. The shell serves it.
2. **Settings source.** `DesktopAppearance` reads `quire/appearance.toml` with a small parser of its
   own (`settings::parse_appearance`: `theme` and `accent` in `[appearance]`; no contrast key exists
   in the document yet). quire's `ds-settings` (v0.3.16) was not added: it pulls `ds-core`, `ds-style`
   (with an image decoder), `chordkit`, `toml` and `notify`, a graph far heavier than three keys. The
   eight accent colours are the macOS system values written into `settings.rs`; check them against
   quire's token table. Changes are found by looking at the file every 2 seconds.
3. **Restore data shape.** Checked against the installed interface XML (version 6): `restore_data`
   is `(suv)`. transom writes `("quire", 1, <token as a string variant>)`; `RestoreToken` is the token.
   Closed.
4. **`UseIn=quire`** in `dist/quire.portal` matches `XDG_CURRENT_DESKTOP`; the session must set
   it to `quire`.
5. **PipeWire needs its development files** to build the `pipewire` crate (libpipewire and clang).
6. **Interface XML re-read (P2-T1, portal 1.22).** Settings, Inhibit and the Screenshot methods match.
   Differences found: Screenshot is version 3 and has an `AvailableTargets` property (now served,
   value 1 = the whole screen; was version 2 without it); the Screenshot options `modal`,
   `interactive`, `target` and `permission_store_checked` are read-only hints and are ignored.
   `dist/` matches the installed `portals.conf(5)` and portal-file format. Still to re-read: ScreenCast
   and RemoteDesktop (P2-T2, P2-T3).
7. **Screenshot cannot list outputs.** `screenshot::OutputLayout` (which outputs exist, where) has only
   `FixedLayout`; `main.rs` passes an empty one, so a screenshot is answered as failed until
   `capture/` offers a listing (names and positions) for the real layout. The Inhibit effect talks to
   the login manager on the system bus (`LogindInhibitor`); the compositor's idle-inhibit is not used.
   `PickColor` answers as cancelled until the shell offers a picker.

## ScreenCast (P2-T2) facts and asks

- **Frame trigger.** The stream is cycle-driven: a frame is written only when one is waiting at the
  next graph cycle. `Stream::trigger_process` (a driver stream that wakes on each captured frame)
  needs the `pipewire` crate feature `v0_3_34`, which is a manifest edit; ask the orchestrator to
  enable it, then drive the stream from `Pump`.
- **Buffers.** Shared memory only on both sides. The compositor's GPU-buffer constraints are
  ignored and a GPU frame is refused by the sink. Enabling them needs a render device and a
  modifier negotiation (a later lane).
- **Cursor.** Hidden and embedded work (embedded asks the compositor to paint the pointer). The
  metadata mode captures without a pointer: no cursor metadata is sent yet.
- **Display.** `DisplayName::session()` (an empty name) connects the way Wayland clients do;
  a name is an absolute socket path. P2-T1's screenshot should use `DisplayName::session()`.
- **Session object.** The service also serves `org.freedesktop.impl.portal.Session` at each
  session path, so the frontend can close a session and the portal can announce `Closed`. The
  `Request` object at a call's handle is not served yet, so a cancel of a pending Start through
  the frontend is not seen.
- **Persist modes 1 and 2** are stored alike; the frontend decides when a mode-1 token is no longer
  handed back.
- **Interface XML.** The ScreenCast signatures match the installed version-6 file; the interface
  property `version` stays 5 because `pipewire-serial` is not sent.

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

## Install (desktop-transom lane, 2026-10-15)

- `scripts/install.sh` added (binary, unit, D-Bus file, portal file; prefix and destdir aware, no home writes).
  The desktop installs transom through it and ships its own `quire-portals.conf` (same choices as `dist/`).
- **Asks before the desktop can drop xdg-desktop-portal-wlr in earnest.** (a) Screenshot is answered as failed
  while `main.rs` passes an empty `FixedLayout` (open item 7): it needs an `OutputLayout` from the capture
  side. (b) ScreenCast Start needs `org.quire.Shell1.PickCaptureSource` served by sill (open item 1);
  without it a browser's share dialog never gets a source. (c) `PickColor` is cancelled until the shell offers
  a picker.

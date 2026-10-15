# Findings

Open items and standing facts. An entry names the condition that closes it.

## Stubs behind frozen interfaces

No `todo!()` left: P2-T1, P2-T2 (with the chooser client) and P2-T3 are filled.

One ignored test: `fill::the_pipewire_sink_accepts_a_run_of_frames` needs a PipeWire daemon; the jailed portal chain runs it.

## Open items

1. **The shell side of the chooser.** `org.quire.Shell1.PickCaptureSource` is the client in `chooser.rs` (the
   `Shell1` proxy): `(app_label: s, kinds: u, multiple: u, cursor_modes: au) -> (sources: a(ss), cursor: u)` on name
   `org.quire.Shell`, path `/org/quire/Shell1`. sill serves it since its p2-ch lane (checked against sill's source on
   2026-10-15, see "transom-shot" below). Closed on this side; the jailed chain's fake shell still stands in for sill.
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
7. **Screenshot outputs.** Closed 2026-10-15 (transom-shot, below): `screenshot::LiveLayout` asks the compositor for
   the outputs at each screenshot. The Inhibit effect talks to the login manager on the system bus
   (`LogindInhibitor`); the compositor's idle-inhibit is not used. `PickColor` is now asked of the shell (below).

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

## transom-shot: screenshot layout, shell asks (2026-10-15)

Not compiled; the orchestrator gates.

- **Layout.** `main.rs` now passes `LiveLayout(DisplayName::session())` and `ExtOpen(DisplayName::session())`. The old
  code passed `$WAYLAND_DISPLAY` as the display name, but a name is an absolute socket path (empty = the session's own),
  so even with a layout every capture would have failed with "a display name must be a socket path". Each screenshot
  connects afresh (`capture::output_facts`), binds every `wl_output` (v1 to v4), reads name, geometry (logical x, y and
  transform), current mode and scale, and hands them to `transom_core::arrange`. Nothing is cached, so outputs that come
  and go, move or change scale are current at the next screenshot.
- **Arrangement (`transom-core/src/layout.rs`).** Logical size = current mode (swapped for the odd transform values)
  over the integer scale, rounded to nearest. The picture is laid out at the finest scale among the outputs: each output
  covers its logical rectangle times that scale, a coarser one is enlarged (nearest pixel) to fit, so screens of mixed
  sharpness share one picture without losing detail. An output with no mode, and a mirror (same place and size as an
  earlier one), is skipped. All outputs together, at their logical positions, is the macOS Cmd+Shift+3 result; the
  portal's `interactive`, `modal` and `target` options stay ignored (nothing is asked; `AvailableTargets` says the whole
  screen), so an interactive request gets the same full-screen picture.
- **Limit: fractional scales.** `xdg_output` (exact logical size and fractional scale) needs the `wayland-protocols`
  feature `unstable`, which is a manifest edit this lane may not make. With `wl_output` alone casement's fractional scale
  arrives as its ceiling (1.5 as 2), so such an output's logical size is under-estimated, the picture is a little small
  and the gap between neighbours is not exact. All-1x and all-2x layouts, and mixes of them, are exact. Ask for the
  orchestrator: enable `wayland-protocols` `unstable` for transom, then bind `zxdg_output_manager_v1` in
  `capture/wl/outputs.rs` and prefer its logical position and size (and use `wp_fractional_scale` or the ratio of mode
  to logical size for the scale).
- **ScreenCast without a shell.** When the shell is not on the bus, or is on it but has not exported
  `/org/quire/Shell1` yet (`UnknownObject`, `UnknownInterface`, `UnknownMethod`), Start is answered as cancelled
  (response 1) and nothing is shared (`chooser::unavailable_is_cancel`). Any other failure of the question (access
  denied, a question already open, a malformed answer) stays response 2. Remote desktop consent keeps its refusal.
- **PickColor.** The Screenshot interface calls `org.quire.Shell1.PickColor` (the `Shell1` proxy); the portal result is
  `color: (ddd)`. No shell, or a cancel, is response 1; a malformed answer is response 2.
- **Acceptance scripts (not run).** `dev/accept/screenshot.sh` needs no shell (Screenshot never asks one) and needs no
  change: it joins the compositor session, so transom's session display is the compositor's; with scale 1 or 2 the
  expected `TRANSOM_OUTPUT_SIZE` is the output's mode size, which is what the picture now has. `dev/accept/
  screencast-output.sh` starts its own fake shell (`start_fake_shell`), as do `chooser.sh` and `screencast-window.sh`; its
  step 1 of `chooser.sh` (no shell) still passes with response 1. `dev/lib/portal_client.py`'s fake shell now also serves
  `PickColor` (a fixed colour, a cancel for `none`); no script exercises it yet.
- **Environment.** transom reads the display the way Wayland clients do (`WAYLAND_DISPLAY`, `XDG_RUNTIME_DIR`), so the
  D-Bus-activated process needs both: the session must run `dbus-update-activation-environment --systemd
  WAYLAND_DISPLAY XDG_RUNTIME_DIR` (or the unit must import them). The screencast path had the same need already.

### Asks for sill

1. **`org.quire.Shell1.PickCaptureSource` is already served** by sill (`sill_services::capture_pick`, exported on the
   seat connection that owns `org.quire.Shell`); its signature, caller check and `("output", name)` / `("window", id)`
   sources match transom's client. Nothing to change. Two notes: (a) until the seat exists the object is absent and
   transom now treats that as "nobody to ask" and cancels; (b) the call has no timeout on either side, it waits on the
   person.
2. **`org.quire.Shell1.PickColor` is not served and is not anywhere in sill.** The shell must serve, on the same object
   (`/org/quire/Shell1`, interface `org.quire.Shell1`, name `org.quire.Shell`):

   ```
   PickColor(app_label: s) -> (color: a(ddd))
   ```
   - `app_label`: the app's name as the person knows it ("Firefox"), the same label `PickCaptureSource` gets.
   - Returns an array of zero or one tuple. Empty is a cancel. One tuple is `(red, green, blue)`, each a double in
     0.0 to 1.0, sRGB. Anything else (two tuples, a channel outside 0 to 1, NaN) is treated as a bad answer and the
     portal call fails.
   - Behaviour: show an on-screen eyedropper over the live screen (magnifier loupe, click to pick, Escape to cancel, as
     macOS Digital Color Meter / the system colour well's eyedropper). Block until the person picks or cancels; no timeout
     (transom waits). One question at a time: a second call while one is open gets `org.freedesktop.DBus.Error.
     LimitsExceeded`. The caller leaving the bus takes the eyedropper down.
   - Caller check, as for `PickCaptureSource`: only the owner of `org.freedesktop.impl.portal.desktop.quire` may call
     (`AccessDenied` otherwise).
   - Until sill serves it, transom answers every `PickColor` as cancelled when the object is absent
     (`UnknownObject`/`UnknownMethod`), so no app breaks.
3. **Remote control consent (from p2-t3, still open).** `AskRemoteControl(app_label: s, devices: u) -> (allowed: u)`
   (1 yes, 0 no), same object and caller check; today transom asks `PickCaptureSource` and reads any pick as yes.
4. **Yes-or-no question (transom-access).** The Access portal needs the person to answer through the shell. The shell
   must serve, on the same object (`/org/quire/Shell1`, interface `org.quire.Shell1`, name `org.quire.Shell`):

   ```
   AskPermission(app_label: s, title: s, subtitle: s, body: s, grant_label: s, deny_label: s) -> (answer: u)
   ```
   - `app_label`: the app's name as the person knows it, as for `PickCaptureSource`. The three texts come from the
     app's request through the portal frontend and are shown as given; any of them may be empty.
   - Behaviour: a sheet (the confirm/consent sheet's look) with the app's icon and name, `title` as the headline,
     `subtitle` under it, `body` as the explanation, and two buttons: `grant_label` (default, Return) and
     `deny_label`. Block until answered; no timeout. One question at a time: a second call while one is open gets
     `org.freedesktop.DBus.Error.LimitsExceeded`. Buttons are disarmed briefly after the sheet appears, as the confirm
     sheet does, so a key typed for something else cannot answer it.
   - `answer`: 0 when the person pressed `grant_label`, 1 when they pressed `deny_label`, 2 when they dismissed the
     sheet (Escape, or it was replaced) without answering. Anything else is a bad answer and the portal call fails.
   - Caller check, as for `PickCaptureSource`: only the owner of `org.freedesktop.impl.portal.desktop.quire`.
   - Until sill serves it, transom answers every question as denied (response 1) when the object is absent, so a
     screenshot is refused rather than taken unasked.
   - The Access portal's `icon` and `choices` options are not forwarded or answered; the reply carries no results.
   - The desktop's own `quire-portals.conf` (desktop/session) must also route
     `org.freedesktop.impl.portal.Access=quire`.

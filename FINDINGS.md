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
  `close`) so tests run without a bus. Seams: `Asker` (the one `Parts.access`; the question is `remote_control_question`) and `EisHandout` (`CasementEis`,
  `FakeEis`); `Parts` gained `eis` (built inside `Parts::real`, whose signature is unchanged).
- **EIS only.** `ConnectToEIS` asks `org.quire.Compositor1.ConnectEis(session: t, devices: u) -> h` (casement; only
  `transom.service` may call it). The `Notify*` methods are accepted and dropped (debug log): there is no second way in.
  Checked against the installed `org.freedesktop.impl.portal.RemoteDesktop.xml`: all signatures match the skeleton; `ConnectToEIS`
  now returns `fdo::Result<h>` so a refusal is an error and not a panic.
- **Session object.** The frontend closes a session through `org.freedesktop.impl.portal.Session.Close` at the session handle; a
  small `SessionObject` is served there (another portal interface serving the same handle is tolerated). No `Closed` signal yet.
- **Closed 2026-10-16 (PORTAL-04): consent is its own question.** remote control now asks `AskPermission` (below) with
  text naming the input kinds in plain words ("keyboard and pointer"); a screen-sharing pick is never a yes. The app is
  shown by its desktop entry's name (PORTAL-11). No `AskRemoteControl` method is needed on the shell's side.
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
3. **Remote control consent (from p2-t3): closed 2026-10-16.** It goes through `AskPermission` (item 4); no
   `AskRemoteControl` is asked of the shell.
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
   - Until sill serves it (it does not at sill 3fecef0), transom answers every question as denied (response 1) when
     the object is absent, so a screenshot is refused rather than taken unasked. Three things now ask it: the Access
     portal, an `interactive` or unvouched Screenshot (PORTAL-06) and remote control (PORTAL-04). Until sill serves
     it, remote control and those screenshots do not work; fail closed.
   - The Access portal's `icon` and `choices` options are not forwarded or answered; the reply carries no results.
   - The desktop's own `quire-portals.conf` (desktop/session) must also route
     `org.freedesktop.impl.portal.Access=quire`.

## transom-repush: a still screen still gives a stream (2026-10-16)

Written without a build or a run: nothing here has been compiled or tried in the jail. Source of the symptoms:
`screencast-output.sh` steps 3, 4 and 5 against casement's headless output; the casement lane's analysis
(cas36) showed casement is correct (a still output has no damage, so the second capture is parked).

**A. Frames from a still source (fixed in transom).**
- `transom_core::pacing` decides: `Negotiated::repeat_gap_ns` (the stream's frame gap, within 1/60 s and 1 s; an
  open rate fills at 5 a second or its ceiling if slower) and `decide(Held, gap) -> Step`. A picture that has not gone
  out is sent at once; a sent one is sent again only after a full gap. Table tests: `transom-core/tests/it/pacing.rs`.
- `screencast/pipewire.rs`: the sink now keeps the newest picture (`Slot`) instead of taking it out on the first
  buffer. `fill_next_buffer` runs `decide` on every process cycle with the gap read from the negotiated format. The
  first frame, pushed before any reader exists, is no longer lost: a reader that links late gets it on its first free
  buffer. A resize clears the held picture; a picture that arrives while another is being written stays new.
- `capture::CaptureSource::next_frame(wait) -> Arrival::{Frame, Quiet}`; `Copier::frame_within` polls the Wayland
  socket (`capture/wl/wait.rs`, `rustix` poll, the crate wayland-client polls with) and keeps the copy request open
  across timeouts, so a change is never missed between calls. `Pump::turn` waits `QUIET_TURN` (250 ms) and reports
  `PumpOutcome::Quiet`.
- `Share::drop` no longer shuts the Wayland socket down; each share thread looks at its stop flag after every turn
  (a quiet turn is at most 250 ms) and closes its own connection. This removes the `Io error: Broken pipe` lines the
  log showed at the end of every share. `Canceller` stays in `capture` for other callers.

**B. "target not found" (consumer `pipewiresrc path=<node>`): cause not established.**
- Read, not run: the id returned is `pw_stream_get_node_id`, set in libpipewire's `proxy_bound_props` just before the
  stream goes Paused, which is when transom reads it; so it is the node's global id. The consumer's `path=` becomes
  `node.target=<id>` (PipeWire 1.6 `pw_stream_connect`), which WirePlumber 0.5 (`find-defined-target.lua`) looks up as
  `node.id` among its session items; `target-object=` is looked up as `object.serial`. So a node id as `path` is
  correct, and the id in the Start response stays the node id as the portal spec says. The "target not found" text
  is WirePlumber's `prepare-link.lua`: no linkable item matched. The node is `Video/Source`, and WirePlumber makes an
  item for `Video/*` nodes (`node/create-item.lua`) once its ports are known.
- So either WirePlumber never made an item for the node (ports not announced, or not running with these scripts in the
  jail) or the item failed `canLink`. Neither can be told from the evidence: no pipewire or wireplumber log was kept.
  Not changed in transom: the properties differ from the other portals' (they set only `media.class` and connect with
  driver and allocate-buffers flags) but none of that is read by the linking scripts.
- To settle it: `dev/lib/chain.sh` now starts WirePlumber with `s-linking` and `s-node` at info level, and
  `dev/lib/cast_check.sh::explain_node` (called by `screencast-output.sh` step 3 on failure) prints the node as the
  graph has it (class, name, `object.serial`, state, port count), the WirePlumber lines naming the id, and whether
  `target-object=<serial>` finds the node when `path=<id>` does not.

**Steps 4 and 5 are not caused by 3.** The portal frontend's app-facing API carries the token as `restore_token`
(`s`), in `SelectSources` options and in the `Start` results; `restore_data` `(suv)` exists only on the backend
interface (see the installed `org.freedesktop.portal.ScreenCast.xml`). `portal_client.py cast` sent and read
`restore_data`, so it could never see a token and never offered one back. Fixed in the client; transom's backend
side (`restore_data` and `persist_mode` in its Start results) was already right.

## Production audit, area PORTAL (2026-10-16, lane pa-transom)

Written without a build or a run: nothing here has been compiled. Fixed in transom: PORTAL-01, 04, 05, 06, 07, 08, 09,
11, 12 (caps), 13 (memfd, node name), 16, 17, 18, 19, 20, 21. Fixed in latchkey: PORTAL-22 to 26.

**Decisions recorded**

- **Restore grants expire (PORTAL-09).** "While running": one day (the portal cannot see the app end, so the longest a
  session of one app plausibly lasts). "Until revoked": thirty days, the interval at which macOS asks again whether an
  app may keep recording the screen. A new grant starts a new period. At most 256 grants; the oldest go first.
  The file format is version 2 (mode and date per line); a version 1 file is dropped without fuss (its grants had no
  date and would expire at once).
- **No identity, no memory (PORTAL-09).** An app with an empty app id (not sandboxed, and the frontend found no desktop
  id for it) is never remembered and a token is never resolved for it. The frontend already passes the best identity it
  has (the sandbox id, or the systemd scope's desktop id).
- **Screenshot consent (PORTAL-06).** Read in xdg-desktop-portal's `desktop-portal/screenshot.c`: for an `interactive`
  request the frontend skips its Access question, and it sets `permission_store_checked` for every request on a
  version 2 or newer backend, interactive or not. So the flag does not vouch for consent by itself. The backend asks
  for every `interactive` request and every request without the flag, and stays quiet only for a non-interactive
  request the frontend vouched for (else two dialogs). Anyone who can call the backend directly can still set the flag:
  that is PORTAL-03, not fixed here.
- **Remote control asks through `AskPermission`**, not a new method (PORTAL-04).
- **Sessions hang on two peers (PORTAL-07).** The caller's unique name and the frontend's at creation. A direct caller
  is not refused (that is PORTAL-03); its sessions end with it.

**Deferred, with the condition that closes each**

- **PORTAL-02, 14, 15** (privileged globals, protocols not served, the accessibility root): a later wave, per the lane
  brief. PORTAL-03 and 10 are closed below (wave 2).
- **PORTAL-12, reusing the read buffer.** Closed 2026-10-17: `capture/pool.rs`. `Copier::read_frame` takes its memory
  from a `FramePool`; the picture carries it as a `PixelBuffer`, which goes back to the pool wherever the picture is
  finally dropped (the sink replacing it, the stream ending), so the sink needed no change beyond the field's type. The
  pool keeps `POOL_KEEP` (3) buffers and lets go of one more than twice the size asked for. The size caps still bound
  a picture.
- **PORTAL-13, restricting the node's readers.** The node is still an ordinary `Video/Source`; its name is now random
  but the graph lists it. Closes with PipeWire access permissions on the node, once the desktop's PipeWire policy
  decides who may link to a capture node.
- **PORTAL-19** is closed by PORTAL-08's bounded waits (a share's thread notices a closed session within a turn,
  a quarter second, and no turn waits on the compositor longer than a deadline). Not separately cancelled.
- **PORTAL-21, `ProtectSystem=strict`.** Not added: a user unit gets a mount namespace only where unprivileged user
  namespaces are allowed, and Ubuntu 24.04 restricts them (owner decision 2026-10-16: no AppArmor allowance there).
  Closes when that changes. `NoNewPrivileges`, `RestrictAddressFamilies=AF_UNIX`, `RestrictSUIDSGID`,
  `LockPersonality` and `SystemCallArchitectures` are in `dist/transom.service`. Casement still trusts the unit name
  `transom.service`, which a user can override with a drop-in.
- **Request objects.** `Request.Close` is still not served for Start, so a cancelled Start leaves the sheet up
  (PORTAL-04's extra).
- **The jailed chain's fake shell** (`dev/`) does not apply the shell's owner check; the private-bus test
  (`tests/it/shell_bus.rs`) does. Closes when the fake shell in `dev/lib/chain.sh` copies the check.
- **Not testable without a compositor:** the deadlines and the size caps in `capture/wl.rs`. The jailed chain's
  `dev/accept/screencast-*.sh` and `screenshot.sh` exercise the happy path; a stalled compositor needs a fixture
  that accepts the connection and never answers.

**Interface asks**

- casement: `org.quire.Compositor1.EndEis(session: t)`: end every socket handed out for `session` (stop its input and
  close the compositor's end), callable only by `transom.service` like `ConnectEis`. Until it exists a close logs a
  warning and the app keeps the socket it was given (PORTAL-05's other half).
- sill: serve `org.quire.Shell1.AskPermission` (item 4 above); remote control and interactive screenshots need it.

## Production audit wave 2, area PORTAL (2026-10-16, lane pa2-transom)

Written without a build or a run: nothing here has been compiled or tested; the tests are written and unrun.
Fixed: PORTAL-03 and PORTAL-10, with `peephole-bus` (tag v0.1.0) as the one way of telling callers apart. See
ARCHITECTURE section 3b-2.

**Decisions recorded**

- **Frontend only, proved `Full` (PORTAL-03).** Every portal method refuses a caller that does not hold role
  `portal_frontend` at `Full`, including the `Notify*` methods (accepted and dropped, still guarded) and Settings.
  The frontend's unit therefore has to be provable by the desktop's table (not ours), and
  `dist/transom.service` unsets the loader variables so transom's own `portal` standing can reach `Full` for the
  casement and sill checks.
- **Opener only (PORTAL-03).** The frontend is one peer for every app, so the "creator" check separates a second
  frontend-role peer from the first and stops a session being driven by anything but its opener; it is not a
  per-app boundary (the frontend owns that).
- **The table is the deployment's (PORTAL-03, 10).** `main.rs` loads `/etc/quire/desktop-callers.toml`. A table that
  is missing, unreadable or not root-only leaves the portal running and refusing everything, with an error in the log.
  This is fail closed on purpose; it also means a machine without the table has no portal.
- **The shell is asked by its unique name (PORTAL-10).** The owner is confirmed and the question is sent to that
  name, so a program that takes the well-known name after the check hears nothing. An owner change while a sheet is
  open ends it as a no (a picker as a cancel).
- **The colour picker goes through the same door.** PORTAL-10 names the picker, the Access sheet and remote
  control; the colour picker asks the same shell and was changed with them.

**Deferred**

- **Properties are not guarded.** zbus answers `Get`/`GetAll` for `version`, `AvailableSourceTypes`,
  `AvailableCursorModes`, `AvailableTargets` itself. They are constants. Closes if a property ever carries state.
- **Sill `DoNotQueue`.** sill requesting `org.quire.Shell` with `DoNotQueue` and failing loudly if it is taken is
  sill's half of PORTAL-10 (not in this repo).
- **The jailed chain** (`dev/`) has no caller table and no `portal_frontend`, so its portal refused everything.
  Closed 2026-10-17: see "The jailed chain is let in" below.
- **No test of the real `Full` path.** Tests introduce standings. Whether the unit manager confirms
  `xdg-desktop-portal.service` on each supported distribution is a gate-run fact (the sample table's exe lists).

**Asks**

- peephole: none. The desktop table must name `portal_frontend` (unit `xdg-desktop-portal.service`) and `shell`
  (`sill.service`); the sample table does.
- sill: serve `org.quire.Shell` with `DoNotQueue` (above).

## The jailed chain is let in (lane devc, 2026-10-17)

The jail has no root-owned table and no unit manager, so every call from `xdg-desktop-portal` was refused as an
unknown caller. peephole v0.1.4 adds developer rows (`dev-rows`: `Table::from_toml_dev`, `TableFiles::load_dev`,
`Proof::Dev` below `Proof::Cgroup`, `Requirement::dev`); transom takes them the way sill does.

- **Feature `debug`** on the transom crate turns on `peephole-bus/dev-rows` and nothing else. It is off by default and
  in every shipped build (the desktop gate greps for `dev-rows`).
- **`TRANSOM_DEV_CALLERS=FILE`**, read in `main.rs` only in that build. `dev_rows::with_dev_rows` loads the person's
  own file and merges it after the shipped table. A file that cannot be taken (a link, writable by others, not a table,
  a row without `exe`, missing) stops the start with the reason, not a portal that quietly refuses its stand-ins.
- **Accepted proof.** `peers::need` lists `Requirement::dev(role)` beside `Proof::Full` for the portal frontend and the
  shell in that build only. That also lets in the real programs at any proof (`Requirement::dev` is "any proof"), so
  `guard.rs`'s weak-proof case is part of the release-shaped build only; `dev_callers.rs` covers the `debug` build.
- **The rows the harness writes** (one per stand-in; the frontend's image is the real `xdg-desktop-portal`):
  `[[peer]]` with `app = "org.freedesktop.portal.Desktop"`, `role = "portal_frontend"`, `exe = ["/usr/libexec/xdg-desktop-portal"]`,
  and one with `app = "org.quire.Shell"`, `role = "shell"`, `exe = [<the stand-in shell's image>]`. A shipped row that
  claims the same image wins over a developer row.
- **Dev scripts.** `dev/lib/chain.sh` passes `TRANSOM_DEV_CALLERS` to transom; no dev script builds transom (they take
  `TRANSOM_BIN`), so the caller builds it with `--features debug` (documented at the top of `chain.sh`).
- **Not verified here (no compile, no run):** the new tests, the build with and without the feature, and the
  `Cargo.lock` move from peephole v0.1.1 to v0.1.4. The workspace `rustix` gains the `process` feature (the person's user
  id), which adds no crate.

## Production audit, wave 3 (2026-10-17)

- **NEW-PI-2: remembering needs a proven app.** See ARCHITECTURE 4d. Residuals: (a) the sandbox proof is peephole's
  metadata-plus-scope check, which a same-user process could imitate with a private mount namespace and a scope of its
  own making; the owner's ruling lets scope claims stand only where the person answers, so this is the weakest part,
  and a stronger proof needs the frontend to say how it learned the id (an ask for xdg-desktop-portal, not ours).
  (b) A host app whose launcher is a script (the entry runs `firefox`, the image is `bash`) is never remembered.
  (c) The restore file is still plain 0600 text in the person's state folder; a token is useless to a program that is
  not shown to be its app, which was the point.
  (d) Unverified: that xdg-dbus-proxy's connection reads as the sandboxed app through its own `.flatpak-info`. If it
  reads as a plain program, no Flatpak app is remembered until that is fixed; the failure is asking too often.
- **NEW-PI-7: revoke path.** `org.quire.Portal1.ListGrants` / `Revoke` (ARCHITECTURE "Taking a remembered share back").
  The Settings page is detent's; the ask is in the lane report: a "Screen sharing" group on the Privacy page listing
  `ListGrants` rows (name, date) with a Remove button calling `Revoke(app_id)`, on the name
  `org.freedesktop.impl.portal.desktop.quire`, path `/org/freedesktop/portal/desktop`. Settings must run as
  `org.quire.Settings.service` from a root-owned unit with the loader variables unset, or it is proved below `Full`
  and refused. Closes when detent has the row.
- **PORTAL-02 rest / NEW-PI-3, transom half.** Nothing in transom trusts a desktop service below `Full`: the frontend,
  the shell and Settings are all asked for at `Full` (`peers::need`). The one `Proof::Exe` left is the unit manager's
  own identity (`ManagerCheck::Verified`, role `manager`), which is a program row with no unit and so cannot be proved
  higher; its answers decide whether others reach `Full`, nothing more.
- **NEW-PI-8: a restart strands handed-out sockets (transom half).** Session numbers now count up from the clock in
  nanoseconds (`Book::starting_after`), so a number is never reused by a later run, and at start `serve_on` calls
  `RemoteDesktopService::forget_previous_run`, which asks the compositor to drop every socket handed to this portal
  before: `org.quire.Compositor1.EndAllEis()`, tolerated when the method or the compositor is absent. The compositor's
  half (the method, or ending a portal's sockets itself when the portal leaves the bus) is casement's; until it lands
  an app of the earlier run keeps its input. Closes when casement has it.
- **NEW-FA-2: the portal's own capture connection (transom half).** A capture on the session's display (an empty
  `DisplayName`) first asks `org.quire.Compositor1.Connect("portal")` over the session bus (`capture::CasementLine`, through the
  connection that owns the portal's name, the way `ConnectEis` is asked, within `LINE_DEADLINE`) and speaks Wayland on the
  descriptor it gets back (`DisplayName::route` gives `Route::Handed`). casement trusts that connection for the screen and the
  windows, so its own Screen sheet is no longer raised over the portal's chooser. When nobody owns the name, the call is refused
  or it times out (another compositor, an older casement), the capture connects the way any Wayland client does (`Route::Env`)
  and the log says so once. A display named by a socket path is used as given and the compositor is not asked. A call over
  casement's rate (four a second) is asked once more after a pause. Closes when casement has `Connect("portal")`
  (casement `fb1-portal-connect`); `desktop/harness/lib/casement-floor.sh` `floor_powers` can then drop its allow row for transom.
- **NEW-FA-2 follow-up: one handed connection per capture job.** The first cut asked for a connection at every step: a
  screenshot asked once for the sources, once for the outputs and once per output captured, a share once per stream. casement
  limits each bus caller to four `Connect` calls in a one-second window (`CALLS_PER_WINDOW`), so a screenshot of three or more
  outputs, or a second one straight after, was refused a handed connection part way, fell back to the public socket (where the
  portal holds no screen power on a real desktop) and failed. Now one capture job (`capture::Job`: a screenshot's grab, or one
  screen-cast Start with its restore lookups and every stream) asks the line once, on its first step, and runs every step on
  that connection (`wl::Wire`): the sources, the output facts and each output or window capture, each on an event queue of its
  own (`wl::Link`) so streams that run at once do not share state. The fall-back to the public socket (no line, or the line
  refused) is decided once for the job, in `Wire::open`, not per step. A failed connection is remembered by the job, so a
  retry of a step does not ask again. A share's captures hold the connection open for as long as they run; the job may go.
  Outputs bound only to be listed are released at the end of the step, since the connection outlives it. Nothing else asks
  per frame or per retry: `DisplayName::route` is called from `Wire::open` only, and a frame that fails is asked for again on
  the same session. The one remaining second ask is `CasementLine`'s own pause-and-retry when casement says it is busy, which
  is for separate jobs arriving together. `Canceller` now cuts the whole job's connection.

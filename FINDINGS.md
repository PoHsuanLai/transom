# Findings

Open items and standing facts. An entry names the condition that closes it.

## Stubs behind frozen interfaces

14 `todo!()`, all in `crates/transom/src/remote_desktop.rs` (P2-T3); `transom-core` has none. P2-T1
and P2-T2 (with the chooser client) are filled.

| File | Items | Lane | Proven by |
| --- | --- | --- | --- |
| `crates/transom/src/remote_desktop.rs` | 14 interface methods | P2-T3 | `dev/accept/remote-desktop.sh` |

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

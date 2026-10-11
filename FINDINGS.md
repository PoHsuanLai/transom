# Findings

Open items and standing facts. An entry names the condition that closes it.

## Stubs behind frozen interfaces

22 `todo!()`, all in the `transom` crate; `transom-core` has none. The fill lanes are P2-T1 and
P2-T3 (P2-T2 and the chooser client are filled) (listed in the program's transom fill-lane notes).

| File | Items | Lane | Proven by |
| --- | --- | --- | --- |
| `crates/transom/src/settings.rs` | `DesktopAppearance::current`, `SettingsService::read_all`, `read` (3) | P2-T1 | `tests/it/fill.rs::the_desktop_appearance_reads_without_error`, `dev/accept/settings.sh` |
| `crates/transom/src/screenshot.rs` | `screenshot`, `pick_color` (2) | P2-T1 | `dev/accept/screenshot.sh` |
| `crates/transom/src/inhibit.rs` | `inhibit`, `create_monitor`, `query_end_response` (3) | P2-T1 | `dev/accept/inhibit.sh` |
| `crates/transom/src/remote_desktop.rs` | 14 interface methods | P2-T3 | `dev/accept/remote-desktop.sh` |

Ignored tests: P2-T1 1 (`crates/transom/tests/it/fill.rs`).

## Open items

1. **The shell side of the chooser does not exist yet.** `org.quire.Shell1.PickCaptureSource` is
   frozen here as the client (`chooser.rs`, the `Shell1` proxy): `(app_label: s, kinds: u,
   multiple: u, cursor_modes: au) -> (sources: a(ss), cursor: u)` on name `org.quire.Shell`, path
   `/org/quire/Shell1`. The shell serves it.
2. **Settings source.** `DesktopAppearance` reads the desktop settings document. Reading it with
   quire's `ds-settings` (v0.3.16, rev `b2dab9ce9b8cb2150b7050b4529d9479f659d3cf`) needs that
   dependency in `crates/transom/Cargo.toml`; P2-T1 adds it there and nowhere else. `transom-core`
   does not need it: the portal's contract is three keys.
3. **Restore data shape.** Checked against the installed interface XML (version 6): `restore_data`
   is `(suv)`. transom writes `("quire", 1, <token as a string variant>)`; `RestoreToken` is the token.
   Closed.
4. **`UseIn=quire`** in `dist/quire.portal` matches `XDG_CURRENT_DESKTOP`; the session must set
   it to `quire`.
5. **PipeWire needs its development files** to build the `pipewire` crate (libpipewire and clang).
6. **Interface XML not re-read.** The method signatures of the five interfaces were written from
   the portal documentation as remembered; the first fill lane diffs them against the installed
   interface files and fixes any difference.

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

# Findings

Open items and standing facts. An entry names the condition that closes it.

## Stubs behind frozen interfaces

27 `todo!()`, all in the `transom` crate; `transom-core` has none. The remaining fill lanes are P2-T2, P2-T3 and the chooser client (listed in the program's transom fill-lane notes).

| File | Items | Lane | Proven by |
| --- | --- | --- | --- |
| `crates/transom/src/screencast/mod.rs` | `create_session`, `select_sources`, `start` (3) | P2-T2 | `dev/accept/screencast-output.sh`, `screencast-window.sh` |
| `crates/transom/src/screencast/pipewire.rs` | `PipewireSink::open`, `push`, `resize` (3) | P2-T2 | `tests/it/fill.rs::the_pipewire_sink_accepts_a_run_of_frames` |
| `crates/transom/src/screencast/restore.rs` | `FileStore::load`, `save` (2) | P2-T2 | `tests/it/fill.rs::the_file_store_round_trips_a_book_and_a_missing_file_is_empty` |
| `crates/transom/src/capture/output.rs` | `OutputCapture::open`, `frame` (2) | P2-T2 | `dev/accept/screencast-output.sh` |
| `crates/transom/src/capture/toplevel.rs` | `ToplevelCapture::open`, `frame` (2) | P2-T2 | `dev/accept/screencast-window.sh` |
| `crates/transom/src/chooser.rs` | `ShellChooser::pick` (1) | P2-T2 (chooser client) | `dev/accept/chooser.sh` |
| `crates/transom/src/remote_desktop.rs` | 14 interface methods | P2-T3 | `dev/accept/remote-desktop.sh` |

Ignored tests: P2-T2 2 (`crates/transom/tests/it/fill.rs`).

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
3. **Restore data shape.** The frontend passes `restore_data` as `(suv)` (vendor, version, data)
   in current interface versions. P2-T2 checks it against the installed interface XML and uses
   `("quire", 1, <token>)`; `RestoreToken` is the token.
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

## Standing facts

- The screen-cast session `step` function is total: every (state, input) pair that matters has
  a row in `crates/transom-core/tests/it/screencast.rs`; anything else leaves the session as it is.
- A restore token resolves all or nothing, and only for the app it was given to.
- No accent chosen is published as an out-of-range colour, as the specification says.

# Findings

Open items and standing facts. An entry names the condition that closes it.

## Stubs behind frozen interfaces

35 `todo!()`, all in the `transom` crate; `transom-core` has none. The fill lanes are P2-T1 to
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
| `crates/transom/src/remote_desktop.rs` | 14 interface methods | P2-T3 | `dev/accept/remote-desktop.sh` |

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

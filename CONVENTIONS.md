# Conventions

transom follows the program's shared conventions: small typed structs, enums instead of `bool`
fields and parameters, typed errors (`thiserror`), no `unsafe`, short files, doc comments on every
public item, tests that never touch the real system. `ARCHITECTURE.md` here has the crate map and
the seams.

1. **Library first.** `transom-core` is pure and usable on its own; `transom` is a library with
   a thin `main.rs`. Every real I/O (pixels in, pixels out, tokens at rest, the chooser, the
   appearance) sits behind a trait with its fake exported from the same crate.
2. **Wire values stay at the edge.** The portal's integers and dictionaries become enums and
   structs in the interface files (`screencast/mod.rs` and its siblings); `transom-core` never
   sees a `u32` that means something except through a `from_wire`/`to_wire` pair.
3. **No panics on a bad request.** A malformed call is a refusal code in the reply, not a panic.
   `unwrap` and `expect` only in tests.
4. **Time, randomness and the environment are arguments.** Only `main.rs` reads the
   environment; restore tokens are minted from bytes the caller drew.
5. **Secrets.** A restore token is a bearer secret: redacted `Debug`, never logged, stored in a
   file readable by its owner only.
6. **Plain language.** Nothing a person sees (the chooser's title, a notification) names a
   protocol, a bus or a bundle id; the app is shown by its own name.
7. **Licence hygiene.** transom is MIT OR Apache-2.0. `xdg-desktop-portal` is a separate
   process we talk to over D-Bus: never linked, never copied. Other portal backends are read for
   facts only (what a key means, what a client expects), never for code; their names stay out of
   the code (`scripts/check-boundary.sh`, row T8). Dependencies must pass `deny.toml`.
8. **`todo!()` lives on master only behind a frozen interface.** Each has a row in
   `FINDINGS.md` naming its fill lane and the test that proves it; that test is written and
   `#[ignore = "<lane>"]`d. A `todo!()` body binds its arguments (`let _ = (a, b);`), and private
   state a frozen body will read carries `#[expect(dead_code, reason = "<lane>")]`.
9. **`#[non_exhaustive]`** on public enums that will grow; `Copy` only for heap-free values of
   at most two words.
10. **`cargo fmt -p <crate>`** to write; the gate runs `cargo fmt --all --check`.
11. **One test binary per crate** (`tests/it/main.rs`), tables over one-off cases.

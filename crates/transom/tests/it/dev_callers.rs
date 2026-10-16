//! `TRANSOM_DEV_CALLERS` (feature `debug`): the person's own rows file names the stand-ins for the
//! portal frontend and the shell. A release build has none of it, so the file is only read here
//! under the feature; the other half is a frontend that a release build refuses at a weak proof
//! (`guard.rs`). Scratch folders only; nothing reads the desktop's table.
#![cfg(feature = "debug")]

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};

use peephole_bus::peephole_core::{Proof, Table, roles};
use transom::dev_rows::with_dev_rows;

use crate::guard::{
    CAST, REQUEST_PATH, as_frontend_at, as_shell_at, call, code, introduce, is_refused, path,
    start_portal,
};

/// What the harness writes: the frontend and the shell, each by the image that stands in for it.
const ROWS: &str = "[[peer]]\napp = \"org.freedesktop.portal.Desktop\"\nrole = \"portal_frontend\"\nexe = [\"/usr/libexec/xdg-desktop-portal\"]\n\n[[peer]]\napp = \"org.quire.Shell\"\nrole = \"shell\"\nexe = [\"/opt/checkout/target/debug/standin-shell\"]\n";

fn write(dir: &Path, name: &str, text: &str, mode: u32) -> PathBuf {
    let file = dir.join(name);
    fs::write(&file, text).expect("a rows file");
    fs::set_permissions(&file, fs::Permissions::from_mode(mode)).expect("its mode");
    file
}

#[test]
fn only_the_persons_own_plain_unshared_file_that_parses_adds_rows() {
    let dir = tempfile::tempdir().expect("scratch");
    let own = write(dir.path(), "own.toml", ROWS, 0o600);
    let link = dir.path().join("link.toml");
    symlink(&own, &link).expect("a link");
    let cases = [
        ("a file of the person's, mode 600", own.clone(), true),
        (
            "writable by the group",
            write(dir.path(), "group.toml", ROWS, 0o660),
            false,
        ),
        ("a link to a good file", link, false),
        (
            "text that is not a table",
            write(dir.path(), "junk.toml", "not = [a table", 0o600),
            false,
        ),
        (
            "a row naming no image",
            write(
                dir.path(),
                "noexe.toml",
                "[[peer]]\napp = \"org.quire.Shell\"\nrole = \"shell\"\n",
                0o600,
            ),
            false,
        ),
        (
            "a file that is not there",
            dir.path().join("none.toml"),
            false,
        ),
    ];
    for (name, file, taken) in cases {
        let got = with_dev_rows(Table::empty(), &file);
        assert_eq!(got.is_ok(), taken, "{name}");
    }
    let table = with_dev_rows(Table::empty(), &own).expect("the good file");
    assert!(
        table.has_role(&roles::PORTAL_FRONTEND),
        "the frontend is named"
    );
    assert!(table.has_role(&roles::SHELL), "the shell is named");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_frontend_named_in_the_persons_rows_is_let_in_at_dev_proof_and_only_as_its_own_role() {
    let Some(portal) = start_portal().await else {
        return;
    };
    let body = |session: &str| {
        (
            path(REQUEST_PATH),
            path(session),
            "org.example.App",
            std::collections::HashMap::<String, zbus::zvariant::OwnedValue>::new(),
        )
    };
    // The frontend at `Proof::Dev`, which only a row of the person's own file gives.
    let frontend = portal.client().await;
    as_frontend_at(&portal.peers, &frontend, Proof::Dev);
    let made = call(
        &frontend,
        CAST,
        "CreateSession",
        &body("/org/freedesktop/portal/desktop/session/x/dev"),
    )
    .await
    .expect("the stand-in frontend is let in");
    assert_eq!(code(&made), 0);

    // The shell's row is no leave to call the portal.
    let shell = portal.client().await;
    as_shell_at(&portal.peers, &shell, Proof::Dev);
    let refused = call(
        &shell,
        CAST,
        "CreateSession",
        &body("/org/freedesktop/portal/desktop/session/x/shell"),
    )
    .await;
    assert!(is_refused(&refused), "{refused:?}");

    // And a caller no row names is still a stranger.
    let stranger = portal.client().await;
    introduce(
        &portal.peers,
        &stranger,
        peephole_bus::peephole::fake::introduced_program("/usr/bin/evil"),
    );
    let refused = call(
        &stranger,
        CAST,
        "CreateSession",
        &body("/org/freedesktop/portal/desktop/session/x/evil"),
    )
    .await;
    assert!(is_refused(&refused), "{refused:?}");
}

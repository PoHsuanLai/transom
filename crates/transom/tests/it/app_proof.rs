//! NEW-PI-2: a share is remembered, and a restore token honoured, only for an app that is shown to
//! be what its id says. A host program that only says the id of an app the person once allowed
//! is asked, every time. A program that gives no id at all is told by the one desktop entry of
//! root's that runs it, and remembered by that entry's id.

use std::sync::Arc;

use peephole_bus::peephole::fake::{introduced_program, introduced_service};
use peephole_bus::peephole_core::{AppKey, Engine, Pin, Proof, Standing, WhyNot};
use transom::app_proof::{
    AppProof, AppProver, PeerProver, RootHeld, TrustAll, Why, judge_app, sender_of,
};
use transom::keeper::{Keeper, Kept, unix_now};
use transom::names::{AppNames, exec_program};
use transom::screencast::restore::MemoryStore;
use transom_core::{CastSource, Keep};

use crate::guard::start_portal;

const SESSION: &str = "/org/freedesktop/portal/desktop/session/1_42/token";

/// A scratch data directory with some desktop entries.
pub fn names_with_entries(entries: &[(&str, &str)]) -> (tempfile::TempDir, AppNames) {
    let dir = tempfile::tempdir().expect("scratch");
    let apps = dir.path().join("applications");
    std::fs::create_dir_all(&apps).expect("folder");
    for (id, entry) in entries {
        std::fs::write(apps.join(format!("{id}.desktop")), entry).expect("entry");
    }
    let names = AppNames::new(vec![dir.path().to_path_buf()], Vec::new());
    (dir, names)
}

/// A scratch data directory with one desktop entry.
fn names_with(id: &str, entry: &str) -> (tempfile::TempDir, AppNames) {
    names_with_entries(&[(id, entry)])
}

const FIREFOX: &str = "[Desktop Entry]\nName=Firefox\nExec=firefox %u\n";

/// An app whose program is `fakeremote` and which says no id of its own.
pub const REMOTE_ID: &str = "org.example.FakeRemote";
pub const REMOTE: &str = "[Desktop Entry]\nName=Fake Remote\nExec=fakeremote %u\n";
pub const REMOTE_PROGRAM: &str = "/usr/share/fakeremote/fakeremote";
const TWIN: &str = "[Desktop Entry]\nName=Twin\nExec=/usr/bin/twin\n";

pub fn program(path: &str) -> Standing {
    introduced_program(path).expect("a program standing")
}

fn sandboxed(app: &str) -> Standing {
    Standing::Sandboxed {
        app: AppKey::new(app).expect("an app key"),
        engine: Engine::Flatpak,
    }
}

#[derive(Debug)]
struct NotRoots;

impl RootHeld for NotRoots {
    fn holds(&self, _path: &std::path::Path) -> bool {
        false
    }
}

#[test]
fn what_a_peer_proves_about_the_app_id_it_is_called_by() {
    let (_dir, names) = names_with("org.mozilla.firefox", FIREFOX);
    let service = introduced_service("org.quire.Shell", "shell", Proof::Full, Pin::AtConnect)
        .expect("a service standing");
    let gone = Standing::Unverified {
        exe_name: None,
        scope_app: None,
        why: WhyNot::PeerGone,
    };
    let table: Vec<(&str, Standing, &str, AppProof)> = vec![
        (
            "a sandbox that names the app",
            sandboxed("org.mozilla.firefox"),
            "org.mozilla.firefox",
            AppProof::Proven("org.mozilla.firefox".to_owned()),
        ),
        (
            "a sandbox that names another app",
            sandboxed("org.evil.Thing"),
            "org.mozilla.firefox",
            AppProof::Unproven(Why::SandboxNamesAnother),
        ),
        (
            "the program the entry runs",
            program("/usr/lib/firefox/firefox"),
            "org.mozilla.firefox",
            AppProof::Proven("org.mozilla.firefox".to_owned()),
        ),
        (
            "a host program that only says an allowed id",
            program("/usr/bin/python3"),
            "org.mozilla.firefox",
            AppProof::Unproven(Why::EntryRunsAnother),
        ),
        (
            "a host program whose id has no entry",
            program("/usr/bin/python3"),
            "org.example.Unregistered",
            AppProof::Unproven(Why::NoRegisteredEntry),
        ),
        (
            "a desktop service",
            service,
            "org.mozilla.firefox",
            AppProof::Unproven(Why::NotAnApp),
        ),
        (
            "a peer that could not be read",
            gone,
            "org.mozilla.firefox",
            AppProof::Unproven(Why::Unidentified),
        ),
    ];
    for (name, standing, id, want) in table {
        assert_eq!(judge_app(&standing, id, &names, &TrustAll), want, "{name}");
    }
}

#[test]
fn an_entry_anyone_but_root_can_write_registers_nothing() {
    let (_dir, names) = names_with("org.mozilla.firefox", FIREFOX);
    let standing = program("/usr/lib/firefox/firefox");
    assert_eq!(
        judge_app(&standing, "org.mozilla.firefox", &names, &NotRoots),
        AppProof::Unproven(Why::NoRegisteredEntry)
    );
}

#[test]
fn the_program_of_an_exec_line() {
    let table: &[(&str, &str, Option<&str>)] = &[
        (
            "plain",
            "[Desktop Entry]\nExec=firefox %u\n",
            Some("firefox"),
        ),
        (
            "a path",
            "[Desktop Entry]\nExec=/usr/bin/gimp-2.10 %U\n",
            Some("/usr/bin/gimp-2.10"),
        ),
        (
            "quoted",
            "[Desktop Entry]\nExec=\"/opt/My App/run\" --x\n",
            Some("/opt/My App/run"),
        ),
        (
            "an action's line is not the entry's",
            "[Desktop Action new]\nExec=other\n[Desktop Entry]\nName=x\n",
            None,
        ),
        ("none", "[Desktop Entry]\nName=x\n", None),
        ("empty", "[Desktop Entry]\nExec=\n", None),
    ];
    for (name, text, want) in table {
        assert_eq!(exec_program(text).as_deref(), *want, "{name}");
    }
}

#[test]
fn the_app_of_a_session_is_read_from_the_handle_the_frontend_made() {
    let table: &[(&str, Option<&str>)] = &[
        (SESSION, Some(":1.42")),
        (
            "/org/freedesktop/portal/desktop/request/1_7/t",
            Some(":1.7"),
        ),
        ("/org/freedesktop/portal/desktop/session/x/token", None),
        ("/org/freedesktop/portal/desktop/session/1_42/", None),
        ("/org/freedesktop/portal/desktop/other/1_42/token", None),
        ("/org/freedesktop/portal/desktop/session/1_4a/token", None),
        ("/session", None),
        ("", None),
    ];
    for (handle, want) in table {
        assert_eq!(sender_of(handle).as_deref(), *want, "{handle}");
    }
}

/// On a bus, through the real prover: the app's connection is looked up by the name in its
/// session handle.
#[tokio::test(flavor = "multi_thread")]
async fn the_real_prover_asks_who_is_on_the_other_end_of_the_session() {
    let Some(portal) = start_portal().await else {
        return;
    };
    let (_dir, names) = names_with("org.mozilla.firefox", FIREFOX);
    let prover = PeerProver::with_root(portal.link.clone(), names, TrustAll);
    portal.peers.introduce(":1.42", program("/usr/bin/python3"));
    assert_eq!(
        prover.prove(SESSION, "org.mozilla.firefox").await,
        AppProof::Unproven(Why::EntryRunsAnother),
        "a host program claiming an allowed id is not remembered"
    );
    portal
        .peers
        .introduce(":1.42", program("/usr/lib/firefox/firefox"));
    assert_eq!(
        prover.prove(SESSION, "org.mozilla.firefox").await,
        AppProof::Proven("org.mozilla.firefox".to_owned())
    );
    portal.peers.introduce(":1.42", program(REMOTE_PROGRAM));
    let (_other, remote_names) = names_with(REMOTE_ID, REMOTE);
    let remote = PeerProver::with_root(portal.link.clone(), remote_names, TrustAll);
    assert_eq!(
        remote.prove(SESSION, "").await,
        AppProof::Proven(REMOTE_ID.to_owned()),
        "a program that gives no id is told by its desktop entry"
    );
    assert_eq!(
        prover.prove("/not/a/session", "org.mozilla.firefox").await,
        AppProof::Unproven(Why::NoHandle)
    );
}

#[test]
fn a_program_that_gives_no_id_is_told_by_the_one_entry_that_runs_it() {
    let (_dir, names) = names_with_entries(&[
        ("org.mozilla.firefox", FIREFOX),
        (REMOTE_ID, REMOTE),
        ("org.example.TwinA", TWIN),
        ("org.example.TwinB", TWIN),
    ]);
    let table: Vec<(&str, Standing, AppProof)> = vec![
        (
            "the one entry that runs it",
            program(REMOTE_PROGRAM),
            AppProof::Proven(REMOTE_ID.to_owned()),
        ),
        (
            "a program no entry runs",
            program("/usr/bin/python3"),
            AppProof::Unproven(Why::NoEntryForProgram),
        ),
        (
            "a program two entries run: nothing is guessed",
            program("/usr/bin/twin"),
            AppProof::Unproven(Why::ProgramNotUnique),
        ),
        (
            "a sandbox that names nothing",
            sandboxed("org.example.Thing"),
            AppProof::Unproven(Why::SandboxNamesAnother),
        ),
    ];
    for (name, standing, want) in table {
        assert_eq!(judge_app(&standing, "", &names, &TrustAll), want, "{name}");
    }
    assert_eq!(
        judge_app(&program(REMOTE_PROGRAM), "", &names, &NotRoots),
        AppProof::Unproven(Why::NoEntryForProgram),
        "an entry anyone but root can write tells nothing"
    );
}

#[test]
fn a_crowded_folder_is_not_searched_to_the_end() {
    let (dir, names) = names_with(REMOTE_ID, REMOTE);
    let apps = dir.path().join("applications");
    for n in 0..4100 {
        std::fs::write(
            apps.join(format!("org.example.Filler{n}.desktop")),
            format!("[Desktop Entry]\nExec=filler{n}\n"),
        )
        .expect("entry");
    }
    assert_eq!(
        judge_app(&program(REMOTE_PROGRAM), "", &names, &TrustAll),
        AppProof::Unproven(Why::ProgramNotUnique)
    );
}

/// The app that gave no id is remembered by its entry's id; the book lists it, restores it and
/// takes it back under that id.
#[tokio::test]
async fn an_app_with_no_id_is_remembered_by_the_id_of_its_entry() {
    let (_dir, names) = names_with(REMOTE_ID, REMOTE);
    let id = judge_app(&program(REMOTE_PROGRAM), "", &names, &TrustAll)
        .id()
        .expect("proven")
        .to_owned();
    let keeper = Arc::new(Keeper::new(Box::new(MemoryStore::default())));
    let screen = vec![CastSource::Output("DP-1".to_owned())];
    let token = keeper
        .keep(&id, Kept::Screen(screen.clone()), Keep::UntilRevoked, None)
        .await
        .expect("kept");
    assert_eq!(
        keeper
            .book()
            .await
            .resolve(&token, &id, &screen, unix_now()),
        screen,
        "the next Start restores it with no question"
    );
    let listed = keeper.remembered().await;
    assert_eq!(listed.len(), 1);
    assert_eq!(
        (listed[0].app_id.as_str(), listed[0].keep),
        (REMOTE_ID, Keep::UntilRevoked)
    );
    assert!(
        keeper
            .keep("", Kept::Screen(screen.clone()), Keep::UntilRevoked, None)
            .await
            .is_none(),
        "an app that is not proven has no id and is not kept"
    );
    assert_eq!(keeper.revoke(REMOTE_ID).await, Some(1));
    assert!(
        keeper
            .book()
            .await
            .resolve(&token, &id, &screen, unix_now())
            .is_empty()
    );
}

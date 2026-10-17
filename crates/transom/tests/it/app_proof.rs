//! NEW-PI-2: a share is remembered, and a restore token honoured, only for an app that is shown to
//! be what its id says. A host program that only says the id of an app the person once allowed
//! is asked, every time.

use peephole_bus::peephole::fake::{introduced_program, introduced_service};
use peephole_bus::peephole_core::{AppKey, Engine, Pin, Proof, Standing, WhyNot};
use transom::app_proof::{
    AppProof, AppProver, PeerProver, RootHeld, TrustAll, Why, judge_app, sender_of,
};
use transom::names::{AppNames, exec_program};

use crate::guard::start_portal;

const SESSION: &str = "/org/freedesktop/portal/desktop/session/1_42/token";

/// A scratch data directory with one desktop entry.
fn names_with(id: &str, entry: &str) -> (tempfile::TempDir, AppNames) {
    let dir = tempfile::tempdir().expect("scratch");
    let apps = dir.path().join("applications");
    std::fs::create_dir_all(&apps).expect("folder");
    std::fs::write(apps.join(format!("{id}.desktop")), entry).expect("entry");
    let names = AppNames::new(vec![dir.path().to_path_buf()], Vec::new());
    (dir, names)
}

const FIREFOX: &str = "[Desktop Entry]\nName=Firefox\nExec=firefox %u\n";

fn program(path: &str) -> Standing {
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
            AppProof::Proven,
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
            AppProof::Proven,
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
        AppProof::Proven
    );
    assert_eq!(
        prover.prove("/not/a/session", "org.mozilla.firefox").await,
        AppProof::Unproven(Why::NoHandle)
    );
}

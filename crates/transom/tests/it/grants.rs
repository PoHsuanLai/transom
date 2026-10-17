//! NEW-PI-7: the person can see what the portal remembers and take it back, through the Settings
//! app and nobody else. On a private bus; callers are introduced with the standing they would have.

use std::time::{SystemTime, UNIX_EPOCH};

use peephole_bus::peephole::fake::{introduced_program, introduced_service};
use peephole_bus::peephole_core::{Pin, Proof};
use transom::screencast::restore::{FileStore, RestoreStore};
use transom::settings::DesktopAppearance;
use transom::{Parts, serve_on};
use transom_core::restore::Grant;
use transom_core::{CastSource, PersistMode, RestoreBook, RestoreToken};
use zbus::Connection;

use crate::bus::PrivateBus;
use crate::guard::{as_frontend, call, introduce, is_refused};

const GRANTS: &str = "org.quire.Portal1";
const APP: &str = "org.example.Meet";

type Listed = Vec<(String, String, u32, u64)>;

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn as_settings_at(portal: &Portal, who: &Connection, proof: Proof) {
    let standing = introduced_service("org.quire.Settings", "settings", proof, Pin::AtConnect);
    introduce(&portal.peers, who, standing);
}

struct Portal {
    bus: PrivateBus,
    peers: std::sync::Arc<transom::peers::Peers>,
    store: FileStore,
    _scratch: tempfile::TempDir,
    _connection: Connection,
}

async fn client(bus: &PrivateBus) -> Connection {
    bus.builder().build().await.expect("a client")
}

/// A portal whose book holds one grant of `APP`.
async fn portal_with_a_grant() -> Option<Portal> {
    let bus = PrivateBus::start()?;
    let scratch = tempfile::tempdir().expect("scratch");
    let path = scratch.path().join("restore");
    let mut book = RestoreBook::new();
    book.remember(
        RestoreToken::mint([9; 16]),
        Grant {
            app_id: APP.to_owned(),
            sources: vec![CastSource::Output("DP-1".to_owned())],
            mode: PersistMode::UntilRevoked,
            granted_at: now(),
        },
    );
    FileStore::new(path.clone()).save(&book).expect("seeded");
    let parts = Parts::real(FileStore::new(path.clone()), DesktopAppearance::new());
    let link = parts.link();
    let connection = serve_on(bus.builder(), parts).await.expect("serves");
    let peers = link.peers().expect("served");
    Some(Portal {
        bus,
        peers,
        store: FileStore::new(path),
        _scratch: scratch,
        _connection: connection,
    })
}

#[tokio::test(flavor = "multi_thread")]
async fn only_settings_lists_and_takes_back_what_is_remembered() {
    let Some(portal) = portal_with_a_grant().await else {
        return;
    };
    let settings = client(&portal.bus).await;
    as_settings_at(&portal, &settings, Proof::Full);

    // The control: Settings sees the app, by its date, with no token in sight.
    let listed: Listed = call(&settings, GRANTS, "ListGrants", &())
        .await
        .expect("settings may list")
        .body()
        .deserialize()
        .expect("a list");
    assert_eq!(listed.len(), 1);
    assert_eq!((listed[0].0.as_str(), listed[0].2), (APP, 2));

    // Nobody else may do either: the frontend, a stranger, Settings proved by its image only.
    let frontend = client(&portal.bus).await;
    as_frontend(&portal.peers, &frontend);
    let stranger = client(&portal.bus).await;
    introduce(
        &portal.peers,
        &stranger,
        introduced_program("/usr/bin/other"),
    );
    #[cfg_attr(feature = "debug", allow(unused_mut))] // only a release build adds the weak proof
    let mut refused = vec![
        (&frontend, "the portal frontend"),
        (&stranger, "a stranger"),
    ];
    // A build with `debug` lists `Proof::Dev` as a way in, which every proof meets.
    #[cfg(not(feature = "debug"))]
    let weak = client(&portal.bus).await;
    #[cfg(not(feature = "debug"))]
    {
        as_settings_at(&portal, &weak, Proof::Exe);
        refused.push((&weak, "settings proved by its image only"));
    }
    for (who, label) in refused {
        assert!(
            is_refused(&call(who, GRANTS, "ListGrants", &()).await),
            "{label} listed"
        );
        assert!(
            is_refused(&call(who, GRANTS, "Revoke", &(APP,)).await),
            "{label} revoked"
        );
    }
    assert_eq!(
        portal.store.load().expect("book").len(),
        1,
        "nothing was taken back"
    );

    // Settings takes it back: the book is changed on disk, and a second time finds nothing.
    let gone: u32 = call(&settings, GRANTS, "Revoke", &(APP,))
        .await
        .expect("settings may revoke")
        .body()
        .deserialize()
        .expect("a count");
    assert_eq!(gone, 1);
    assert!(portal.store.load().expect("book").is_empty());
    let again: u32 = call(&settings, GRANTS, "Revoke", &(APP,))
        .await
        .expect("answers")
        .body()
        .deserialize()
        .expect("a count");
    assert_eq!(again, 0);
    let listed: Listed = call(&settings, GRANTS, "ListGrants", &())
        .await
        .expect("answers")
        .body()
        .deserialize()
        .expect("a list");
    assert!(listed.is_empty());
}

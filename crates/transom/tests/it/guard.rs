//! Who may call the portal, and whom the portal asks, on a private bus. Callers are introduced
//! with the standing they would have (the portal frontend, the shell, a stranger); nothing reads
//! `/proc` or the unit manager. Each test first shows the control (the right caller is let
//! through), so a refusal proves the check and not a broken setup.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use peephole_bus::peephole::fake::{introduced_program, introduced_service};
use peephole_bus::peephole_core::{Pin, Proof, Standing};
use transom::link::BusLink;
use transom::peers::Peers;
use transom::screencast::restore::FileStore;
use transom::settings::DesktopAppearance;
use transom::{BUS_NAME, PORTAL_PATH, Parts, serve_on};
use zbus::zvariant::{ObjectPath, OwnedValue};
use zbus::{Connection, Message, fdo};

use crate::bus::PrivateBus;

const SETTLE: Duration = Duration::from_secs(5);

type Dict = HashMap<String, OwnedValue>;

/// The unique name of a connection.
fn name_of(connection: &Connection) -> String {
    connection
        .unique_name()
        .map(ToString::to_string)
        .unwrap_or_default()
}

pub fn introduce(peers: &Peers, who: &Connection, standing: Option<Standing>) {
    peers.introduce(&name_of(who), standing.expect("a valid standing"));
}

/// Make `who` stand as the portal frontend, proved fully.
pub fn as_frontend(peers: &Peers, who: &Connection) {
    as_frontend_at(peers, who, Proof::Full);
}

/// Make `who` stand as the portal frontend, proved only as far as `proof`.
pub fn as_frontend_at(peers: &Peers, who: &Connection, proof: Proof) {
    let standing = introduced_service(
        "org.freedesktop.portal.Desktop",
        "portal_frontend",
        proof,
        Pin::AtConnect,
    );
    introduce(peers, who, standing);
}

/// Make `who` stand as the shell, proved fully.
pub fn as_shell(peers: &Peers, who: &Connection) {
    as_shell_at(peers, who, Proof::Full);
}

/// Make `who` stand as the shell, proved only as far as `proof`.
pub fn as_shell_at(peers: &Peers, who: &Connection, proof: Proof) {
    let standing = introduced_service("org.quire.Shell", "shell", proof, Pin::AtConnect);
    introduce(peers, who, standing);
}

/// Whether the call was refused as a caller that is not allowed.
pub fn is_refused<T>(result: &zbus::Result<T>) -> bool {
    const NAMES: [&str; 2] = [
        "org.freedesktop.DBus.Error.AccessDenied",
        "org.freedesktop.portal.Error.NotAllowed",
    ];
    match result {
        Err(zbus::Error::MethodError(name, _, _)) => NAMES.contains(&name.as_str()),
        Err(zbus::Error::FDO(error)) => matches!(**error, fdo::Error::AccessDenied(_)),
        _ => false,
    }
}

/// The portal on a private bus, and the means to introduce callers to it.
pub struct Portal {
    pub bus: PrivateBus,
    _portal: Connection,
    pub link: BusLink,
    pub peers: Arc<Peers>,
}

pub async fn start_portal() -> Option<Portal> {
    let bus = PrivateBus::start()?;
    let scratch = tempfile::tempdir().expect("scratch");
    let parts = Parts::real(
        FileStore::new(scratch.path().join("restore")),
        DesktopAppearance::new(),
    );
    let link = parts.link();
    let connection = serve_on(bus.builder(), parts)
        .await
        .expect("the portal serves");
    let peers = link.peers().expect("callers can be told apart once served");
    Some(Portal {
        bus,
        _portal: connection,
        link,
        peers,
    })
}

impl Portal {
    pub async fn client(&self) -> Connection {
        self.bus.builder().build().await.expect("a client")
    }

    async fn frontend(&self) -> Connection {
        let connection = self.client().await;
        as_frontend(&self.peers, &connection);
        connection
    }
}

pub async fn call<B>(
    who: &Connection,
    interface: &str,
    method: &str,
    body: &B,
) -> zbus::Result<Message>
where
    B: zbus::export::serde::ser::Serialize + zbus::zvariant::DynamicType,
{
    tokio::time::timeout(
        SETTLE,
        who.call_method(Some(BUS_NAME), PORTAL_PATH, Some(interface), method, body),
    )
    .await
    .expect("the portal answered in time")
}

pub fn path(text: &str) -> ObjectPath<'static> {
    ObjectPath::try_from(text.to_owned()).expect("an object path")
}

/// A reply of the shape `(u, a{sv})`: the response code.
pub fn code(reply: &Message) -> u32 {
    reply
        .body()
        .deserialize::<(u32, Dict)>()
        .expect("a response")
        .0
}

pub const CAST: &str = "org.freedesktop.impl.portal.ScreenCast";
const REMOTE: &str = "org.freedesktop.impl.portal.RemoteDesktop";
const INHIBIT: &str = "org.freedesktop.impl.portal.Inhibit";
const SESSION: &str = "org.freedesktop.impl.portal.Session";
pub const REQUEST_PATH: &str = "/org/freedesktop/portal/desktop/request/x/r";

/// Every method of every portal interface, called as `who`; each with its label.
async fn every_method(who: &Connection) -> Vec<(&'static str, zbus::Result<Message>)> {
    let (handle, session) = (
        path(REQUEST_PATH),
        path("/org/freedesktop/portal/desktop/session/x/s"),
    );
    let none = Dict::new();
    let app = "org.example.App";
    let shot = "org.freedesktop.impl.portal.Screenshot";
    vec![
        (
            "Screenshot",
            call(who, shot, "Screenshot", &(&handle, app, "", &none)).await,
        ),
        (
            "PickColor",
            call(who, shot, "PickColor", &(&handle, app, "", &none)).await,
        ),
        (
            "ScreenCast.CreateSession",
            call(who, CAST, "CreateSession", &(&handle, &session, app, &none)).await,
        ),
        (
            "ScreenCast.SelectSources",
            call(who, CAST, "SelectSources", &(&handle, &session, app, &none)).await,
        ),
        (
            "ScreenCast.Start",
            call(who, CAST, "Start", &(&handle, &session, app, "", &none)).await,
        ),
        (
            "RemoteDesktop.CreateSession",
            call(
                who,
                REMOTE,
                "CreateSession",
                &(&handle, &session, app, &none),
            )
            .await,
        ),
        (
            "RemoteDesktop.SelectDevices",
            call(
                who,
                REMOTE,
                "SelectDevices",
                &(&handle, &session, app, &none),
            )
            .await,
        ),
        (
            "RemoteDesktop.Start",
            call(who, REMOTE, "Start", &(&handle, &session, app, "", &none)).await,
        ),
        (
            "RemoteDesktop.NotifyPointerMotion",
            call(
                who,
                REMOTE,
                "NotifyPointerMotion",
                &(&session, &none, 1.0_f64, 1.0_f64),
            )
            .await,
        ),
        (
            "RemoteDesktop.ConnectToEIS",
            call(who, REMOTE, "ConnectToEIS", &(&session, app, &none)).await,
        ),
        (
            "Access.AccessDialog",
            call(
                who,
                "org.freedesktop.impl.portal.Access",
                "AccessDialog",
                &(&handle, app, "", "t", "s", "b", &none),
            )
            .await,
        ),
        (
            "Inhibit.Inhibit",
            call(who, INHIBIT, "Inhibit", &(&handle, app, "", 8_u32, &none)).await,
        ),
        (
            "Inhibit.CreateMonitor",
            call(who, INHIBIT, "CreateMonitor", &(&handle, &session, app, "")).await,
        ),
        (
            "Inhibit.QueryEndResponse",
            call(who, INHIBIT, "QueryEndResponse", &(&session,)).await,
        ),
        (
            "Settings.Read",
            call(
                who,
                "org.freedesktop.impl.portal.Settings",
                "Read",
                &("org.freedesktop.appearance", "color-scheme"),
            )
            .await,
        ),
        (
            "Settings.ReadAll",
            call(
                who,
                "org.freedesktop.impl.portal.Settings",
                "ReadAll",
                &(Vec::<String>::new(),),
            )
            .await,
        ),
    ]
}

#[tokio::test(flavor = "multi_thread")]
async fn only_the_portal_frontend_may_call_any_method() {
    let Some(portal) = start_portal().await else {
        return;
    };
    // The control: the frontend is let through, so the refusals below are the check.
    let frontend = portal.frontend().await;
    let created = call(
        &frontend,
        CAST,
        "CreateSession",
        &(
            path(REQUEST_PATH),
            path("/org/freedesktop/portal/desktop/session/x/control"),
            "org.example.App",
            Dict::new(),
        ),
    )
    .await
    .expect("the frontend is let through");
    assert_eq!(code(&created), 0);

    // A program that is in no row, the shell itself (right table, wrong role), and a frontend
    // proved only by its image.
    let stranger = portal.client().await;
    introduce(
        &portal.peers,
        &stranger,
        introduced_program("/usr/bin/evil"),
    );
    let shell = portal.client().await;
    as_shell(&portal.peers, &shell);
    #[cfg_attr(feature = "debug", allow(unused_mut))] // only a release build adds the weak proof
    let mut refused = vec![("a stranger", &stranger), ("the shell", &shell)];
    // A build with `debug` lists `Proof::Dev` as a way in, and every proof is at least that, so
    // a frontend proved by its image alone is let in there; `dev_callers.rs` covers that build.
    #[cfg(not(feature = "debug"))]
    let weak = portal.client().await;
    #[cfg(not(feature = "debug"))]
    {
        as_frontend_at(&portal.peers, &weak, Proof::Exe);
        refused.push(("a weak proof", &weak));
    }
    for (who, caller) in refused {
        for (label, result) in every_method(caller).await {
            assert!(is_refused(&result), "{who} calling {label}: {result:?}");
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_is_closed_and_changed_only_by_the_peer_that_opened_it() {
    let Some(portal) = start_portal().await else {
        return;
    };
    let (opener, other) = (portal.frontend().await, portal.frontend().await);
    let (handle, none) = (path(REQUEST_PATH), Dict::new());
    let cast = "/org/freedesktop/portal/desktop/session/x/cast";
    let remote = "/org/freedesktop/portal/desktop/session/x/remote";
    let monitor = "/org/freedesktop/portal/desktop/session/x/monitor";
    for (interface, session) in [(CAST, cast), (REMOTE, remote)] {
        let made = call(
            &opener,
            interface,
            "CreateSession",
            &(&handle, path(session), "org.example.App", &none),
        )
        .await
        .expect("created");
        assert_eq!(code(&made), 0, "{interface}");
    }
    let made = call(
        &opener,
        INHIBIT,
        "CreateMonitor",
        &(&handle, path(monitor), "org.example.App", ""),
    )
    .await
    .expect("a monitor");
    assert_eq!(made.body().deserialize::<u32>().expect("a code"), 0);

    // Another frontend-held peer changes and ends nothing it did not open.
    let select = |session: &str| {
        (
            path(REQUEST_PATH),
            path(session),
            "org.example.App",
            Dict::new(),
        )
    };
    assert!(is_refused(
        &call(&other, CAST, "SelectSources", &select(cast)).await
    ));
    assert!(is_refused(
        &call(&other, REMOTE, "SelectDevices", &select(remote)).await
    ));
    assert!(is_refused(
        &call(&other, INHIBIT, "QueryEndResponse", &(path(monitor),)).await
    ));
    for session in [cast, remote, monitor] {
        let closed = other
            .call_method(Some(BUS_NAME), session, Some(SESSION), "Close", &())
            .await;
        assert!(is_refused(&closed), "{session}: {closed:?}");
    }

    // The sessions survived, and their opener still has them.
    let kept = call(&opener, CAST, "SelectSources", &select(cast))
        .await
        .expect("kept");
    assert_eq!(code(&kept), 0, "the session is still open");
    call(&opener, REMOTE, "SelectDevices", &select(remote))
        .await
        .expect("kept");
    call(&opener, INHIBIT, "QueryEndResponse", &(path(monitor),))
        .await
        .expect("kept");
    for session in [cast, remote, monitor] {
        opener
            .call_method(Some(BUS_NAME), session, Some(SESSION), "Close", &())
            .await
            .unwrap_or_else(|error| panic!("the opener closes {session}: {error}"));
    }
    let gone = call(&opener, CAST, "SelectSources", &select(cast))
        .await
        .expect("answered");
    assert_eq!(code(&gone), 2, "a closed session is gone");
}

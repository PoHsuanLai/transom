//! Who a session or a hold belongs to, and ending it when they leave the bus.
//!
//! A share, a remote-control session or a hold on the screen's sleep must not outlive the people
//! it was made for. Each is tied to two peers: the one that made the call and the portal frontend
//! that was on the bus then. When either unique name leaves the bus, or the frontend gives up its
//! well-known name, everything tied to it ends. Counts are capped per caller and in total, so
//! one peer cannot fill the maps.
//!
//! Before any of that, every call to a portal interface must come from the portal frontend:
//! [`FrontendOnly`] asks [`peephole_bus`] who the sender is and refuses anyone but the program the
//! caller table names as the frontend. A session or request also remembers the peer that made it
//! ([`Creator`]) and only that peer may end or change it.

use std::future::poll_fn;
use std::pin::pin;
use std::time::Duration;

use peephole_bus::peephole::RealProc;
use peephole_bus::peephole_core::{Proof, Refused, Requirement, Table, roles};
use peephole_bus::{BusPeers, ManagerCheck, ManagerLink, Tokio, access_denied};
use zbus::export::futures_core::Stream;
use zbus::message::Header;
use zbus::names::BusName;
use zbus::{Connection, fdo};

use crate::inhibit::InhibitService;
use crate::link::BusLink;
use crate::remote_desktop::RemoteDesktopService;
use crate::screencast::ScreenCastService;
use crate::{PORTAL_PATH, TransomError};

/// How callers on the bus are told apart: their process, held by a pidfd, judged against the
/// caller table. The probe runs on tokio's blocking pool.
pub type Peers = BusPeers<RealProc, Tokio, ManagerLink<RealProc>>;

/// [`Peers`] on `connection`, judging against `table`. The unit manager's answers are believed
/// only when the manager's own image is the system's (role `manager`, proved by its image).
pub fn peers_on(connection: Connection, table: Table) -> Peers {
    let manager = ManagerCheck::Verified(Requirement::new(roles::MANAGER, Proof::Exe));
    BusPeers::new(connection, table, manager).with_offload(Tokio)
}

/// The well-known name of the portal frontend, the peer that relays every app's calls.
pub const FRONTEND_NAME: &str = "org.freedesktop.portal.Desktop";

/// How long to wait for the bus to say who the frontend is. The bus answers at once; a longer
/// wait means it is stuck, and the session is then tied to its caller only.
const OWNER_LOOKUP: Duration = Duration::from_secs(2);

/// The most sessions of one kind open at once, over all callers.
pub const SESSION_LIMITS: Limits = Limits {
    total: 64,
    per_caller: 16,
};

/// The most holds on the session's sleep open at once. An app holds one or two.
pub const HOLD_LIMITS: Limits = Limits {
    total: 128,
    per_caller: 32,
};

/// How long a session that never began may sit before a full book makes room by dropping it.
pub const UNSTARTED_TTL: Duration = Duration::from_secs(15 * 60);

/// How many may be open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// Over all callers.
    pub total: usize,
    /// For one caller.
    pub per_caller: usize,
}

/// Whether one more may be opened.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Admission {
    /// Yes.
    Allowed,
    /// No: the caller or the whole book is at its limit.
    Full,
}

/// The peers something hangs on: whoever made the call, and the portal frontend at the time.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tether {
    caller: Option<String>,
    frontend: Option<String>,
}

impl Tether {
    /// A tether to a caller and a frontend, by unique name.
    pub fn new(caller: Option<String>, frontend: Option<String>) -> Self {
        Self { caller, frontend }
    }

    /// The tether of a call that arrived with `header` on `connection`.
    pub async fn of(connection: &Connection, header: &Header<'_>) -> Self {
        let caller = header.sender().map(ToString::to_string);
        let frontend = tokio::time::timeout(OWNER_LOOKUP, owner_of(connection, FRONTEND_NAME))
            .await
            .ok()
            .flatten();
        Self { caller, frontend }
    }

    /// The peer that made the call this tether is for.
    pub fn creator(&self) -> Creator {
        Creator(self.caller.clone())
    }

    /// Whether `peer` (a unique name) is one of the two.
    pub fn holds(&self, peer: &str) -> bool {
        self.caller.as_deref() == Some(peer) || self.frontend.as_deref() == Some(peer)
    }

    /// Whether both were made by the same known caller.
    pub fn same_caller(&self, other: &Self) -> bool {
        self.caller.is_some() && self.caller == other.caller
    }
}

/// The peer that made a session or a request. Only that peer may end it or change it; a peer that
/// cannot be named matches nobody.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Creator(Option<String>);

impl Creator {
    /// The creator of the session a call with `header` opens: its sender.
    pub fn of(header: &Header<'_>) -> Self {
        Self(header.sender().map(ToString::to_string))
    }

    /// `Ok` when the call with `header` comes from this creator, `AccessDenied` otherwise.
    pub fn require_caller(&self, header: &Header<'_>) -> fdo::Result<()> {
        match (&self.0, header.sender()) {
            (Some(made), Some(now)) if made == now.as_str() => Ok(()),
            _ => Err(fdo::Error::AccessDenied(
                "this was opened by another caller".to_owned(),
            )),
        }
    }
}

/// The guard every portal method starts with: the caller must be the portal frontend, proved
/// at the strongest level (its image and its unit, as the unit manager states it). Before
/// [`crate::serve`] has attached the means of telling callers apart, nobody passes.
#[derive(Clone, Debug, Default)]
pub struct FrontendOnly {
    link: BusLink,
}

impl FrontendOnly {
    /// A guard that identifies callers through `link`.
    pub fn new(link: BusLink) -> Self {
        Self { link }
    }

    /// Let the call with `header` through if it comes from the portal frontend. The result is
    /// the creator to record on a session or request the call opens.
    pub async fn admit(&self, header: &Header<'_>) -> fdo::Result<Creator> {
        let Some(peers) = self.link.peers() else {
            tracing::warn!("a call arrived before callers could be told apart, so it is refused");
            return Err(access_denied(Refused::Unknown));
        };
        let need = [Requirement::new(roles::PORTAL_FRONTEND, Proof::Full)];
        let sender = header.sender().map(|s| s.as_str());
        match peers.require(sender, &need).await {
            Ok(_) => Ok(Creator::of(header)),
            Err(refused) => {
                tracing::warn!(%refused, ?sender, "a call that is not from the portal frontend was refused");
                Err(access_denied(refused))
            }
        }
    }

    /// As [`FrontendOnly::admit`], and the call must also come from `creator`: the peer that
    /// opened the session or request it is about.
    pub async fn admit_creator(&self, header: &Header<'_>, creator: &Creator) -> fdo::Result<()> {
        self.admit(header).await?;
        creator.require_caller(header)
    }
}

/// Whether a newcomer with `tether` fits among the `open` ones.
pub fn admit<'a>(
    open: impl IntoIterator<Item = &'a Tether>,
    newcomer: &Tether,
    limits: Limits,
) -> Admission {
    let (mut total, mut theirs) = (0_usize, 0_usize);
    for tether in open {
        total += 1;
        if tether.same_caller(newcomer) {
            theirs += 1;
        }
    }
    if total >= limits.total || theirs >= limits.per_caller {
        Admission::Full
    } else {
        Admission::Allowed
    }
}

async fn owner_of(connection: &Connection, name: &str) -> Option<String> {
    let dbus = fdo::DBusProxy::new(connection).await.ok()?;
    let name = BusName::try_from(name).ok()?;
    dbus.get_name_owner(name).await.ok().map(|o| o.to_string())
}

/// Which unique name a change of name ownership takes off the bus, if any: a unique name that lost
/// its owner (the peer left), or the frontend's well-known name changing hands (the old owner is
/// no longer the frontend, whether or not it is still connected).
pub fn departed(name: &str, old_owner: Option<&str>, new_owner: Option<&str>) -> Option<String> {
    if name.starts_with(':') && new_owner.is_none() {
        return Some(name.to_owned());
    }
    if name == FRONTEND_NAME && new_owner != old_owner {
        return old_owner.map(ToOwned::to_owned);
    }
    None
}

/// Watch the bus on `connection` and end everything tied to a peer that leaves. The subscription
/// is made before this returns, so no departure after it is missed.
pub async fn spawn_watch(connection: &Connection) -> Result<(), TransomError> {
    let dbus = fdo::DBusProxy::new(connection).await?;
    let changes = dbus.receive_name_owner_changed().await?;
    let connection = connection.clone();
    tokio::spawn(async move {
        let mut changes = pin!(changes);
        while let Some(change) = poll_fn(|cx| changes.as_mut().poll_next(cx)).await {
            let Ok(args) = change.args() else { continue };
            let old = args.old_owner().as_ref().map(|o| o.as_str());
            let new = args.new_owner().as_ref().map(|o| o.as_str());
            if let Some(peer) = departed(args.name().as_str(), old, new) {
                release(&connection, &peer).await;
            }
        }
    });
    Ok(())
}

/// End what is tied to `peer` in every service that ties things to peers.
async fn release(connection: &Connection, peer: &str) {
    let server = connection.object_server();
    if let Ok(cast) = server.interface::<_, ScreenCastService>(PORTAL_PATH).await {
        cast.get().await.peer_gone(peer, connection).await;
    }
    if let Ok(remote) = server
        .interface::<_, RemoteDesktopService>(PORTAL_PATH)
        .await
    {
        remote.get().await.peer_gone(peer, connection).await;
    }
    if let Ok(inhibit) = server.interface::<_, InhibitService>(PORTAL_PATH).await {
        inhibit.get().await.peer_gone(peer, connection).await;
    }
}

//! Input sockets from the compositor, and taking them back.

use std::fmt;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use super::devices::DeviceKinds;
use crate::BoxFuture;
use crate::link::BusLink;

/// How long the compositor has to hand out or take back a socket before the attempt is dropped:
/// it is a local call that answers at once, so a longer wait means it is stuck.
pub const EIS_DEADLINE: Duration = Duration::from_secs(10);

/// Why an EIS socket could not be had.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EisError {
    /// The compositor is not on the bus.
    #[error("the compositor is not available")]
    Unavailable,
    /// The compositor refused or the call failed.
    #[error("the compositor did not hand out a connection: {0}")]
    Call(String),
}

/// The seam: a socket the app speaks EIS on, for one session, and the end of it.
pub trait EisHandout: Send + Sync + fmt::Debug {
    /// The client end of an EIS socket for portal session `session` offering `kinds`.
    fn connect<'a>(
        &'a self,
        session: u64,
        kinds: DeviceKinds,
    ) -> BoxFuture<'a, Result<OwnedFd, EisError>>;

    /// Take back every socket handed out for `session`: the compositor stops its input and closes
    /// its end. Best effort (a compositor that is gone has nothing left to stop); failures are
    /// logged here.
    fn end<'a>(&'a self, session: u64) -> BoxFuture<'a, ()>;

    /// Take back every socket an earlier run of this portal handed out: this portal has just
    /// started and holds no session, so the compositor must hold no input of its. Best effort,
    /// like [`EisHandout::end`]. The default does nothing.
    fn end_all<'a>(&'a self) -> BoxFuture<'a, ()> {
        Box::pin(std::future::ready(()))
    }
}

impl<T: EisHandout + ?Sized> EisHandout for Arc<T> {
    fn connect<'a>(
        &'a self,
        session: u64,
        kinds: DeviceKinds,
    ) -> BoxFuture<'a, Result<OwnedFd, EisError>> {
        (**self).connect(session, kinds)
    }

    fn end<'a>(&'a self, session: u64) -> BoxFuture<'a, ()> {
        (**self).end(session)
    }

    fn end_all<'a>(&'a self) -> BoxFuture<'a, ()> {
        (**self).end_all()
    }
}

/// The compositor's side of the handshake, as the client sees it.
///
/// `ConnectEis(session: t, devices: u) -> (h)`: only this portal's unit may call it. The reply
/// is the client end of an EIS socket whose input the compositor tags as remote input of
/// `session`; `devices` is the portal's `types` bit set.
///
/// `EndEis(session: t)`: ends every socket handed out for `session`, so its input stops and its
/// peer sees the connection close. Called when the session is closed.
///
/// `Connect(role: s) -> (h)`: a Wayland connection with the role `role`. This portal asks for
/// `"portal"` for its captures (`capture::CasementLine`): the connection is trusted for the screen
/// and the windows, so the compositor does not put its own question over the portal's.
#[zbus::proxy(
    interface = "org.quire.Compositor1",
    default_service = "org.quire.Compositor1",
    default_path = "/org/quire/Compositor1",
    gen_blocking = false
)]
pub trait Compositor1 {
    /// An EIS socket for a remote-desktop session.
    fn connect_eis(&self, session: u64, devices: u32) -> zbus::Result<zbus::zvariant::OwnedFd>;

    /// A Wayland connection with the role `role`, when the caller holds the role that stands for it.
    fn connect(&self, role: &str) -> zbus::Result<zbus::zvariant::OwnedFd>;

    /// End every socket handed out for a session.
    fn end_eis(&self, session: u64) -> zbus::Result<()>;

    /// End every socket handed out to this portal before now (it has just started).
    fn end_all_eis(&self) -> zbus::Result<()>;
}

/// The real hand-out: `org.quire.Compositor1.ConnectEis` over the session bus, asked from the
/// connection that owns the portal's name (the compositor tells its caller by that connection).
#[derive(Debug)]
pub struct CasementEis {
    link: BusLink,
}

impl CasementEis {
    /// A hand-out that asks through `link`.
    pub fn new(link: BusLink) -> Self {
        Self { link }
    }

    async fn proxy(&self) -> Result<Compositor1Proxy<'static>, EisError> {
        let call = |error: String| EisError::Call(error);
        let connection = self
            .link
            .connection()
            .map_err(|error| call(error.to_string()))?;
        Compositor1Proxy::new(&connection)
            .await
            .map_err(|error| call(error.to_string()))
    }
}

impl EisHandout for CasementEis {
    fn connect<'a>(
        &'a self,
        session: u64,
        kinds: DeviceKinds,
    ) -> BoxFuture<'a, Result<OwnedFd, EisError>> {
        Box::pin(async move {
            let asked = async {
                let proxy = self.proxy().await?;
                match proxy.connect_eis(session, kinds.to_wire()).await {
                    Ok(fd) => Ok(OwnedFd::from(fd)),
                    Err(zbus::Error::MethodError(name, ..))
                        if name.as_str().ends_with("ServiceUnknown")
                            || name.as_str().ends_with("NameHasNoOwner") =>
                    {
                        Err(EisError::Unavailable)
                    }
                    Err(error) => Err(EisError::Call(error.to_string())),
                }
            };
            tokio::time::timeout(EIS_DEADLINE, asked)
                .await
                .unwrap_or_else(|_| Err(EisError::Call("the compositor did not answer".to_owned())))
        })
    }

    fn end<'a>(&'a self, session: u64) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let asked = async {
                let proxy = self.proxy().await?;
                proxy
                    .end_eis(session)
                    .await
                    .map_err(|error| EisError::Call(error.to_string()))
            };
            match tokio::time::timeout(EIS_DEADLINE, asked).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => tracing::warn!(%error, "an input socket could not be taken back"),
                Err(_) => {
                    tracing::warn!("the compositor did not take an input socket back in time")
                }
            }
        })
    }

    fn end_all<'a>(&'a self) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            match tokio::time::timeout(EIS_DEADLINE, self.end_all_now()).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    tracing::warn!(%error, "input sockets of an earlier run could not be taken back")
                }
                Err(_) => tracing::warn!(
                    "the compositor did not take back the input sockets of an earlier run in time"
                ),
            }
        })
    }
}

impl CasementEis {
    async fn end_all_now(&self) -> Result<(), EisError> {
        let proxy = self.proxy().await?;
        match proxy.end_all_eis().await {
            Ok(()) => Ok(()),
            // A compositor that is not there has nothing to end; one that drops sockets by itself
            // when the portal leaves the bus does not offer this call.
            Err(zbus::Error::MethodError(name, ..))
                if ["ServiceUnknown", "NameHasNoOwner", "UnknownMethod"]
                    .iter()
                    .any(|tail| name.as_str().ends_with(tail)) =>
            {
                Ok(())
            }
            Err(error) => Err(EisError::Call(error.to_string())),
        }
    }
}

/// A hand-out that makes socket pairs: it keeps the server ends so a test can talk through
/// them, and records what was asked and what was taken back.
#[derive(Debug, Default)]
pub struct FakeEis {
    asked: Mutex<Vec<(u64, DeviceKinds)>>,
    servers: Mutex<Vec<(u64, UnixStream)>>,
    ended: Mutex<Vec<u64>>,
    ended_all: Mutex<usize>,
}

impl FakeEis {
    /// A hand-out with nothing asked yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// The asks so far: the session number and the kinds.
    pub fn asked(&self) -> Vec<(u64, DeviceKinds)> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The sessions whose sockets were taken back, in order.
    pub fn ended(&self) -> Vec<u64> {
        self.ended
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// How many times every socket of an earlier run was taken back.
    pub fn ended_all(&self) -> usize {
        *self
            .ended_all
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Takes the server end of the most recent socket.
    pub fn take_server(&self) -> Option<UnixStream> {
        self.servers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop()
            .map(|(_, server)| server)
    }
}

impl EisHandout for FakeEis {
    fn connect<'a>(
        &'a self,
        session: u64,
        kinds: DeviceKinds,
    ) -> BoxFuture<'a, Result<OwnedFd, EisError>> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((session, kinds));
        let made = UnixStream::pair().map_err(|error| EisError::Call(error.to_string()));
        Box::pin(async move {
            let (server, client) = made?;
            self.servers
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push((session, server));
            Ok(OwnedFd::from(client))
        })
    }

    /// Records the end and closes the server ends of the session's sockets that were not taken, so
    /// the client end reads end-of-file, as it would from a compositor that stopped.
    fn end<'a>(&'a self, session: u64) -> BoxFuture<'a, ()> {
        self.ended
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(session);
        self.servers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|(id, _)| *id != session);
        Box::pin(std::future::ready(()))
    }

    /// Records that every earlier socket was taken back and closes all the server ends.
    fn end_all<'a>(&'a self) -> BoxFuture<'a, ()> {
        *self
            .ended_all
            .lock()
            .unwrap_or_else(PoisonError::into_inner) += 1;
        self.servers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        Box::pin(std::future::ready(()))
    }
}

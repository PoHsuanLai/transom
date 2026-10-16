//! What holding a request does to the session: the login manager's inhibitor lock.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use transom_core::InhibitFlags;

use crate::BoxFuture;

/// How long the login manager has to answer before the attempt is dropped. It answers at once; a
/// hung one must not hold every other hold and release behind it.
pub const LOGIND_DEADLINE: Duration = Duration::from_secs(5);

/// Why the session could not be told.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EffectError {
    /// The service that holds the inhibition refused or could not be reached.
    #[error("the session could not be told: {0}")]
    Session(String),
}

/// The seam: what holding a request does to the session. Called only when the union of held
/// flags changes, with the new union ([`InhibitFlags::NONE`] releases everything).
pub trait InhibitEffect: Send + std::fmt::Debug {
    /// Make the session hold exactly `active`.
    fn apply(&mut self, active: InhibitFlags) -> BoxFuture<'_, Result<(), EffectError>>;
}

/// The words logind uses for what to hold off, from the portal's flags. Logging out maps to
/// `shutdown`; switching users has no logind counterpart and is not held.
pub fn logind_what(active: InhibitFlags) -> String {
    [
        (InhibitFlags::LOGOUT, "shutdown"),
        (InhibitFlags::SUSPEND, "sleep"),
        (InhibitFlags::IDLE, "idle"),
    ]
    .iter()
    .filter(|(flag, _)| active.contains(*flag))
    .map(|(_, word)| *word)
    .collect::<Vec<_>>()
    .join(":")
}

/// The real effect: a blocking inhibitor lock taken from the system's login manager. The lock is
/// the file descriptor it hands back; dropping it releases.
#[derive(Debug, Default)]
pub struct LogindInhibitor {
    connection: Option<zbus::Connection>,
    lock: Option<zbus::zvariant::OwnedFd>,
}

impl LogindInhibitor {
    /// An inhibitor that connects to the system bus on first use.
    pub fn new() -> Self {
        Self::default()
    }

    async fn take(&mut self, active: InhibitFlags) -> Result<(), EffectError> {
        match tokio::time::timeout(LOGIND_DEADLINE, self.take_within(active)).await {
            Ok(result) => result,
            Err(_) => Err(EffectError::Session(
                "the login manager did not answer in time".to_owned(),
            )),
        }
    }

    async fn take_within(&mut self, active: InhibitFlags) -> Result<(), EffectError> {
        let session = |e: &dyn std::fmt::Display| EffectError::Session(e.to_string());
        let what = logind_what(active);
        if what.is_empty() {
            self.lock = None;
            return Ok(());
        }
        let connection = match &self.connection {
            Some(c) => c.clone(),
            None => {
                let c = zbus::Connection::system().await.map_err(|e| session(&e))?;
                self.connection = Some(c.clone());
                c
            }
        };
        let reply = connection
            .call_method(
                Some("org.freedesktop.login1"),
                "/org/freedesktop/login1",
                Some("org.freedesktop.login1.Manager"),
                "Inhibit",
                &(
                    what.as_str(),
                    "Apps",
                    "An app asked to keep the session running",
                    "block",
                ),
            )
            .await
            .map_err(|e| session(&e))?;
        let fd: zbus::zvariant::OwnedFd = reply.body().deserialize().map_err(|e| session(&e))?;
        // The new lock is held before the old one is let go, so there is no gap.
        self.lock = Some(fd);
        Ok(())
    }
}

impl InhibitEffect for LogindInhibitor {
    fn apply(&mut self, active: InhibitFlags) -> BoxFuture<'_, Result<(), EffectError>> {
        Box::pin(self.take(active))
    }
}

/// An effect for tests: records each union it was asked to hold.
#[derive(Debug, Default)]
pub struct FakeEffect {
    applied: Arc<Mutex<Vec<InhibitFlags>>>,
}

impl FakeEffect {
    /// An effect that has applied nothing.
    pub fn new() -> Self {
        Self::default()
    }

    /// The log of applied unions, shared with the effect, so a test can keep it after the effect
    /// moved into a hub.
    pub fn log(&self) -> Arc<Mutex<Vec<InhibitFlags>>> {
        Arc::clone(&self.applied)
    }
}

impl InhibitEffect for FakeEffect {
    fn apply(&mut self, active: InhibitFlags) -> BoxFuture<'_, Result<(), EffectError>> {
        self.applied
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(active);
        Box::pin(std::future::ready(Ok(())))
    }
}

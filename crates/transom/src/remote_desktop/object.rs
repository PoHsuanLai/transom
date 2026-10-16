//! The backend's end of a remote-control session, served at the session handle.

use std::sync::Arc;

use zbus::fdo;
use zbus::message::Header;

use super::sessions::Core;
use crate::peers::FrontendOnly;
use crate::screencast::session::SessionKey;

/// The backend's end of a session, at the session handle: the frontend calls `Close` on it.
#[derive(Debug)]
pub(super) struct SessionObject {
    core: Arc<Core>,
    key: SessionKey,
    guard: FrontendOnly,
}

impl SessionObject {
    /// The object for the session `key`, whose callers `guard` admits.
    pub(super) fn new(core: Arc<Core>, key: SessionKey, guard: FrontendOnly) -> Self {
        Self { core, key, guard }
    }
}

#[zbus::interface(name = "org.freedesktop.impl.portal.Session")]
impl SessionObject {
    /// End the session and take its input socket back. Only the peer that opened the session
    /// may end it.
    async fn close(
        &self,
        #[zbus(object_server)] server: &zbus::ObjectServer,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        match self.core.creator(&self.key) {
            Some(creator) => self.guard.admit_creator(&header, &creator).await?,
            None => {
                self.guard.admit(&header).await?;
            }
        }
        self.core.close(&self.key).await;
        let _ = server.remove::<SessionObject, _>(self.key.0.as_str()).await;
        Ok(())
    }

    /// The interface version.
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        1
    }
}

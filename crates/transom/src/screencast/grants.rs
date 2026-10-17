//! `org.quire.Portal1`: the person's view of what the portal remembers, for the Settings app.
//!
//! A share that is remembered "until revoked" needs a way to be revoked (NEW-PI-7). Settings lists
//! the apps that may share the screen without asking again and takes any of them back. Only the
//! Settings app may call: it is proved `Full` like the portal frontend ([`SettingsOnly`]). Nothing
//! secret leaves: the list names apps and dates, never tokens.

use std::sync::Arc;

use zbus::fdo;
use zbus::message::Header;

use super::flow::Shared;
use crate::names::AppNames;
use crate::peers::SettingsOnly;

/// The longest app id taken back (an id is a file name, so it cannot be longer).
const MAX_ID_BYTES: usize = 255;

/// The served interface.
#[derive(Debug)]
pub struct GrantsService {
    shared: Arc<Shared>,
    guard: SettingsOnly,
    names: AppNames,
}

impl GrantsService {
    pub(super) fn new(shared: Arc<Shared>, guard: SettingsOnly, names: AppNames) -> Self {
        Self {
            shared,
            guard,
            names,
        }
    }
}

#[zbus::interface(name = "org.quire.Portal1")]
impl GrantsService {
    /// The apps that may share the screen without asking again, one entry each:
    /// `(app id, name to show, mode, granted at)`. The mode is the portal's persist mode (1 while
    /// the app runs, 2 until taken back); the date is seconds since the Unix epoch.
    async fn list_grants(
        &self,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<Vec<(String, String, u32, u64)>> {
        self.guard.admit(&header).await?;
        Ok(self
            .shared
            .remembered()
            .await
            .into_iter()
            .map(|app| {
                let label = self.names.label(&app.app_id);
                (app.app_id, label, app.mode.to_wire(), app.granted_at)
            })
            .collect())
    }

    /// Take back everything remembered for `app_id`: it is asked again next time. Replies with how
    /// many grants went. A running share is not stopped.
    async fn revoke(&self, app_id: &str, #[zbus(header)] header: Header<'_>) -> fdo::Result<u32> {
        self.guard.admit(&header).await?;
        if app_id.is_empty() || app_id.len() > MAX_ID_BYTES {
            return Err(fdo::Error::InvalidArgs("not an app".to_owned()));
        }
        match self.shared.revoke(app_id).await {
            Some(gone) => Ok(u32::try_from(gone).unwrap_or(u32::MAX)),
            None => Err(fdo::Error::Failed(
                "what is remembered could not be changed".to_owned(),
            )),
        }
    }
}

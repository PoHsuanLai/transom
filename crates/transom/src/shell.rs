//! Asking the shell, and only the shell.
//!
//! The picker, the yes-or-no sheet and the colour picker are surfaces of the desktop shell, which
//! answers on the well-known name [`SHELL_NAME`]. A name can be taken by any program while the shell
//! restarts, so each question is put in four steps and nothing is remembered between questions:
//!
//! 1. Subscribe to changes of the name's owner.
//! 2. Look up the owner and require that it is the shell, proved by its image and its unit.
//! 3. Ask that owner by its unique name, so the question cannot reach whoever takes the name next.
//! 4. If the name changes hands while the person is deciding, drop the question: it is a no.
//!
//! Any step that fails is a refusal ([`ChooserError::is_refusal`]), never a yes.

use std::future::Future;

use peephole_bus::OwnerWatch;
use peephole_bus::peephole_core::roles;

use crate::chooser::{ChooserError, Shell1Proxy, call_failed, nobody_answers};
use crate::link::BusLink;
use crate::peers::need;

/// The well-known name the shell answers on.
pub const SHELL_NAME: &str = "org.quire.Shell";

/// The shell, confirmed for one question: a client addressed to its unique name, and the watch
/// that fires when the name changes hands.
#[derive(Debug)]
pub(crate) struct Door {
    /// The shell's interface, on the confirmed owner.
    pub shell: Shell1Proxy<'static>,
    /// Fires when the owner of [`SHELL_NAME`] changes.
    pub watch: OwnerWatch,
}

impl Door {
    /// Confirm who owns [`SHELL_NAME`] now. A name nobody owns, an owner that is not the shell and
    /// a bus that does not answer are all [`ChooserError::NotTheShell`].
    pub async fn open(link: &BusLink) -> Result<Self, ChooserError> {
        let connection = link.connection()?;
        let Some(peers) = link.peers() else {
            tracing::warn!("the shell cannot be confirmed before callers can be told apart");
            return Err(ChooserError::NotTheShell);
        };
        let watch = peers.watch_owner(SHELL_NAME).await.map_err(|error| {
            tracing::warn!(%error, "the shell's name could not be watched, so it is not asked");
            ChooserError::NotTheShell
        })?;
        let owner = peers
            .owner_of(SHELL_NAME, &need(roles::SHELL))
            .await
            .map_err(|refused| {
                tracing::warn!(%refused, "the shell could not be confirmed, so it is not asked");
                ChooserError::NotTheShell
            })?;
        let shell = Shell1Proxy::builder(&connection)
            .destination(owner)
            .map_err(call_failed)?
            .build()
            .await
            .map_err(call_failed)?;
        Ok(Self { shell, watch })
    }
}

/// Wait for `call` to the confirmed shell, unless the name changes hands first
/// ([`ChooserError::ShellChanged`]). A shell that does not serve the question at all is
/// [`ChooserError::Unavailable`].
pub(crate) async fn answered<T>(
    watch: &mut OwnerWatch,
    call: impl Future<Output = zbus::Result<T>>,
) -> Result<T, ChooserError> {
    tokio::select! {
        biased;
        _ = watch.changed() => {
            tracing::warn!("the shell changed while it was being asked, so the question ends");
            Err(ChooserError::ShellChanged)
        }
        asked = call => match asked {
            Ok(answer) => Ok(answer),
            Err(error) if nobody_answers(&error) => Err(ChooserError::Unavailable),
            Err(error) => Err(call_failed(error)),
        },
    }
}

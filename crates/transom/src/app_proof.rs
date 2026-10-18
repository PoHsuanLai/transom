//! Whether an app is who it says it is, before the portal remembers what it was allowed
//! (NEW-PI-2).
//!
//! The portal frontend passes an app id with every call. For a sandboxed app the frontend read it
//! from the sandbox; for a host app it is the app's own word (it registers it, or names its
//! launcher scope). A share remembered under such a word can be claimed by any program of the
//! same user that says the same word, and it would stream the screen with no sheet. So a restore
//! token is honoured, and a new one issued, only for an app that is shown to be what its id says:
//!
//! - a sandboxed app (its sandbox's metadata agrees with its control group, which `peephole`
//!   checks), whose sandbox names the id; or
//! - a host app whose image is root-owned and is the program the id's desktop entry, written by
//!   root, says to run.
//!
//! Any other app, a host app that only declares its id included, gets the sheet every time.
//!
//! The app is found from the session handle: the frontend builds it as
//! `/org/freedesktop/portal/desktop/session/<sender>/<token>` from the real sender of the app's
//! call, so the app's bus name is read from the path of a call the frontend (proved `Full`) made,
//! never from anything the app wrote.

use std::fmt;
use std::path::Path;

use peephole_bus::peephole::{ProcFs, RealProc, dirs_trust, weaker};
use peephole_bus::peephole_core::{ExePath, FileTrust, Standing};

use crate::BoxFuture;
use crate::link::BusLink;
use crate::names::{AppNames, Registered};

/// Why an app is not remembered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Why {
    /// Nothing can tell apps apart yet, or nothing was set up to.
    NoProver,
    /// The session handle does not name the app's bus connection.
    NoHandle,
    /// The app's connection could not be identified.
    Unidentified,
    /// The app is a desktop service or something else that is not an ordinary app.
    NotAnApp,
    /// The sandbox names another app than the id says.
    SandboxNamesAnother,
    /// The app is a host program and its id has no desktop entry of root's.
    NoRegisteredEntry,
    /// The id's desktop entry runs another program than the one calling.
    EntryRunsAnother,
}

impl fmt::Display for Why {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoProver => "nothing tells apps apart",
            Self::NoHandle => "the session names no app",
            Self::Unidentified => "the app could not be identified",
            Self::NotAnApp => "not an ordinary app",
            Self::SandboxNamesAnother => "its sandbox names another app",
            Self::NoRegisteredEntry => "no desktop entry of root's registers the id",
            Self::EntryRunsAnother => "the id's desktop entry runs another program",
        })
    }
}

/// Whether what an app was allowed may be remembered, or honoured again.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AppProof {
    /// The app is what its id says.
    Proven,
    /// The app is not shown to be what its id says; it is asked every time.
    Unproven(Why),
}

/// Decides whether an app is what its id says.
pub trait AppProver: Send + Sync + fmt::Debug {
    /// The proof for the app of `session` (a session handle the frontend made) calling itself
    /// `app_id`.
    fn prove<'a>(&'a self, session: &'a str, app_id: &'a str) -> BoxFuture<'a, AppProof>;
}

/// Proves nobody: every app is asked every time. What a service without a prover does.
#[derive(Clone, Copy, Debug, Default)]
pub struct NoProof;

impl AppProver for NoProof {
    fn prove<'a>(&'a self, _session: &'a str, _app_id: &'a str) -> BoxFuture<'a, AppProof> {
        Box::pin(async { AppProof::Unproven(Why::NoProver) })
    }
}

/// Whether a file is one only root can have written.
pub trait RootHeld: Send + Sync + fmt::Debug {
    /// `true` when the file at `path` and every folder above it is root's alone.
    fn holds(&self, path: &Path) -> bool;
}

/// The machine's file system.
#[derive(Clone, Copy, Debug, Default)]
pub struct RealRoot;

impl RootHeld for RealRoot {
    fn holds(&self, path: &Path) -> bool {
        let fs = RealProc::system();
        let file = fs.trust_of(path);
        let above = dirs_trust(&fs, path);
        matches!((file, above), (Ok(file), Ok(above)) if weaker(above, file) == FileTrust::RootOnly)
    }
}

/// Every path is root's. For tests.
#[derive(Clone, Copy, Debug, Default)]
pub struct TrustAll;

impl RootHeld for TrustAll {
    fn holds(&self, _path: &Path) -> bool {
        true
    }
}

/// Proves an app from the program on the other end of its bus connection.
#[derive(Debug)]
pub struct PeerProver {
    link: BusLink,
    names: AppNames,
    root: Box<dyn RootHeld>,
}

impl PeerProver {
    /// A prover asking through `link`, finding desktop entries with `names`.
    pub fn new(link: BusLink, names: AppNames) -> Self {
        Self::with_root(link, names, RealRoot)
    }
}

impl PeerProver {
    /// As [`PeerProver::new`], with `root` deciding which files are root's.
    pub fn with_root(link: BusLink, names: AppNames, root: impl RootHeld + 'static) -> Self {
        Self {
            link,
            names,
            root: Box::new(root),
        }
    }
}

impl AppProver for PeerProver {
    fn prove<'a>(&'a self, session: &'a str, app_id: &'a str) -> BoxFuture<'a, AppProof> {
        Box::pin(async move {
            let Some(sender) = sender_of(session) else {
                return AppProof::Unproven(Why::NoHandle);
            };
            let Some(peers) = self.link.peers() else {
                return AppProof::Unproven(Why::NoProver);
            };
            match peers.who(&sender).await {
                Ok(standing) => judge_app(&standing, app_id, &self.names, self.root.as_ref()),
                Err(error) => {
                    tracing::info!(%error, "the app could not be identified, so it is not remembered");
                    AppProof::Unproven(Why::Unidentified)
                }
            }
        })
    }
}

/// What a peer's standing proves about the app id it is called by.
pub fn judge_app(
    standing: &Standing,
    app_id: &str,
    names: &AppNames,
    root: &dyn RootHeld,
) -> AppProof {
    match standing {
        Standing::Sandboxed { app, .. } if app.as_str() == app_id => AppProof::Proven,
        Standing::Sandboxed { .. } => AppProof::Unproven(Why::SandboxNamesAnother),
        Standing::Program { exe, .. } => match names.registered(app_id, root) {
            Registered::Program(program) if runs(&program, exe) => AppProof::Proven,
            Registered::Program(_) => AppProof::Unproven(Why::EntryRunsAnother),
            _ => AppProof::Unproven(Why::NoRegisteredEntry),
        },
        Standing::Service { .. } => AppProof::Unproven(Why::NotAnApp),
        _ => AppProof::Unproven(Why::Unidentified),
    }
}

/// Whether the desktop entry's `program` is the image `exe`: the same file name (an entry names a
/// program by its path or through the search path, and the image is where that resolved to).
fn runs(program: &str, exe: &ExePath) -> bool {
    let named = Path::new(program)
        .file_name()
        .and_then(|name| name.to_str());
    named.is_some() && named == exe.file_name()
}

/// The bus name of the app a session handle belongs to: the path ends in
/// `.../<sender>/<token>`, where `<sender>` is the app's unique name without its colon and with
/// dots written as underscores (`:1.42` is `1_42`).
pub fn sender_of(session: &str) -> Option<String> {
    let mut parts = session.rsplit('/');
    let token = parts.next().filter(|t| !t.is_empty())?;
    let sender = parts.next()?;
    let kind = parts.next()?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let shaped = sender.split('_').count() >= 2 && sender.split('_').all(digits);
    (shaped && matches!(kind, "session" | "request") && !token.contains('\0'))
        .then(|| format!(":{}", sender.replace('_', ".")))
}

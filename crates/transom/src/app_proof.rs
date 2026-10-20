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
//! An unsandboxed app that gives no id at all (a program started by a system service, which no
//! launcher scope names) is told by its program instead: when exactly one desktop entry written
//! by root runs the program calling, that entry's id is the app's. Several or none leave it
//! unproven; nothing is guessed. The id a proof names is the one the portal then remembers by.
//!
//! The app is found from the session handle: the frontend builds it as
//! `/org/freedesktop/portal/desktop/session/<sender>/<token>` from the real sender of the app's
//! call, so the app's bus name is read from the path of a call the frontend (proved `Full`) made,
//! never from anything the app wrote.

use std::fmt;
use std::path::Path;

use peephole_bus::peephole::{RealProc, path_trust};
use peephole_bus::peephole_core::{ExePath, FileTrust, Standing};

use crate::BoxFuture;
use crate::link::BusLink;
use crate::names::{AppNames, Found, Registered};

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
    /// The app gave no id and no desktop entry of root's runs its program.
    NoEntryForProgram,
    /// The app gave no id and more than one desktop entry of root's runs its program (or there
    /// are too many entries to tell).
    ProgramNotUnique,
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
            Self::NoEntryForProgram => "no desktop entry of root's runs this program",
            Self::ProgramNotUnique => "more than one desktop entry of root's runs this program",
        })
    }
}

/// Whether what an app was allowed may be remembered, or honoured again.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AppProof {
    /// The app is what its id says; the id is the one to remember it by. It is the id the app
    /// gave, or, for an app that gave none, the id of the entry that runs its program.
    Proven(String),
    /// The app is not shown to be what its id says; it is asked every time.
    Unproven(Why),
}

impl AppProof {
    /// The id the app is proven to have, if it is.
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Proven(id) => Some(id),
            Self::Unproven(_) => None,
        }
    }
}

/// Decides whether an app is what its id says, or which app it is when it says nothing.
pub trait AppProver: Send + Sync + fmt::Debug {
    /// The proof for the app of `session` (a session handle the frontend made) calling itself
    /// `app_id`, which is empty for an app that gave none.
    fn prove<'a>(&'a self, session: &'a str, app_id: &'a str) -> BoxFuture<'a, AppProof>;
}

/// The app of a call: how it is shown, and the id it is remembered by if it is proven.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Who {
    /// The id to show the person (the proven id, else the one the app gave, which may be empty).
    pub shown: String,
    /// The proven id; `None` for an app that is asked every time.
    pub proven: Option<String>,
}

impl Who {
    /// Who the app of `session` calling itself `app_id` is, as `prover` shows.
    pub async fn identify(prover: &dyn AppProver, session: &str, app_id: &str) -> Self {
        match prover.prove(session, app_id).await {
            AppProof::Proven(id) => Self {
                shown: id.clone(),
                proven: Some(id),
            },
            AppProof::Unproven(why) => {
                tracing::info!(%why, "this app is asked every time: it is not shown to be what its id says");
                Self {
                    shown: app_id.to_owned(),
                    proven: None,
                }
            }
        }
    }
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
        matches!(
            path_trust(&RealProc::system(), path),
            Ok(FileTrust::RootOnly)
        )
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
        Standing::Sandboxed { app, .. } if app.as_str() == app_id => {
            AppProof::Proven(app_id.to_owned())
        }
        Standing::Sandboxed { .. } => AppProof::Unproven(Why::SandboxNamesAnother),
        Standing::Program { exe, .. } if app_id.is_empty() => by_program(exe, names, root),
        Standing::Program { exe, .. } => match names.registered(app_id, root) {
            Registered::Program(program) if runs(&program, exe) => {
                AppProof::Proven(app_id.to_owned())
            }
            Registered::Program(_) => AppProof::Unproven(Why::EntryRunsAnother),
            _ => AppProof::Unproven(Why::NoRegisteredEntry),
        },
        Standing::Service { .. } => AppProof::Unproven(Why::NotAnApp),
        _ => AppProof::Unproven(Why::Unidentified),
    }
}

/// The app a program is, when it gave no id: the one desktop entry of root's that runs it.
fn by_program(exe: &ExePath, names: &AppNames, root: &dyn RootHeld) -> AppProof {
    match names.running(root, |program| runs(program, exe)) {
        Found::One(id) => AppProof::Proven(id),
        Found::Nobody => AppProof::Unproven(Why::NoEntryForProgram),
        Found::Several | Found::TooMany => AppProof::Unproven(Why::ProgramNotUnique),
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

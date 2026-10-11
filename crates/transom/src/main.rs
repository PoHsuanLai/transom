//! The `transom` binary: parse nothing, build the real seams, serve until killed.

use std::path::PathBuf;
use std::process::ExitCode;

use transom::chooser::ShellChooser;
use transom::screencast::restore::FileStore;
use transom::settings::DesktopAppearance;
use transom::{Parts, serve};

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let parts = Parts::real(
        ShellChooser::new(),
        FileStore::new(state_file()),
        DesktopAppearance::new(),
    );
    match serve(parts).await {
        Ok(_connection) => {
            std::future::pending::<()>().await;
            ExitCode::SUCCESS
        }
        Err(error) => {
            tracing::error!(%error, "the portal could not start");
            ExitCode::FAILURE
        }
    }
}

/// Where restore tokens are kept: `$XDG_STATE_HOME/transom/restore`, or under `$HOME/.local/state`.
/// The only place the environment is read; the libraries take the path.
fn state_file() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/state")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("transom").join("restore")
}

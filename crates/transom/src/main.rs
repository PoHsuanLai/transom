//! The `transom` binary: parse nothing, build the real seams, serve until killed.

use std::path::PathBuf;
use std::process::ExitCode;

use transom::capture::DisplayName;
use transom::chooser::ShellChooser;
use transom::screencast::restore::FileStore;
use transom::screenshot::{ExtOpen, LiveLayout, OutputGrab, ScreenshotSetup, ShellColorPicker};
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
        DesktopAppearance::at(appearance_file()),
    )
    .with_screenshots(screenshots());
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

/// Where the desktop keeps its appearance: `$XDG_CONFIG_HOME/quire/appearance.toml`, or under
/// `$HOME/.config`.
fn appearance_file() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("quire").join("appearance.toml")
}

/// Screenshots: the compositor the session's own display names (found the way Wayland clients
/// find theirs), files in `$HOME/Pictures`, named with the time the portal started. The outputs
/// are asked of the compositor at each screenshot, so a screen plugged in or out is accounted
/// for. The colour picker is the shell's.
fn screenshots() -> ScreenshotSetup {
    let pictures = std::env::var_os("HOME")
        .map(|h| PathBuf::from(h).join("Pictures"))
        .unwrap_or_default();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default();
    ScreenshotSetup::new(
        Box::new(OutputGrab::new(
            Box::new(LiveLayout(DisplayName::session())),
            Box::new(ExtOpen(DisplayName::session())),
        )),
        pictures,
        stamp,
    )
    .with_picker(Box::new(ShellColorPicker::new()))
}

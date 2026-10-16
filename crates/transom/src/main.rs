//! The `transom` binary: parse nothing, build the real seams, serve until killed.

use std::path::PathBuf;
use std::process::ExitCode;

use transom::capture::DisplayName;
use transom::link::BusLink;
use transom::names::AppNames;
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
        FileStore::new(state_file()),
        DesktopAppearance::at(appearance_file()),
    )
    .with_names(app_names());
    let parts = {
        let setup = screenshots(parts.link());
        parts.with_screenshots(setup)
    };
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

/// Where an app's own name is found: the desktop entries in the data directories (the person's, the
/// flatpak exports, then the system's), preferring the person's language.
fn app_names() -> AppNames {
    let absolute = |p: &PathBuf| p.is_absolute();
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(absolute)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    let system = std::env::var("XDG_DATA_DIRS")
        .ok()
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(home) = data_home {
        dirs.push(home.join("flatpak/exports/share"));
        dirs.insert(0, home);
    }
    dirs.extend(system.split(':').map(PathBuf::from).filter(absolute));
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share"));
    AppNames::new(dirs, locales())
}

/// The person's locales, most specific first: `zh_TW`, then `zh`. None for the plain C locale.
fn locales() -> Vec<String> {
    let chosen = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|key| std::env::var(key).ok())
        .find(|value| !value.is_empty());
    let Some(value) = chosen else {
        return Vec::new();
    };
    let full = value.split(['.', '@']).next().unwrap_or("").to_owned();
    let language = full.split('_').next().unwrap_or("").to_owned();
    let mut found = vec![full];
    found.push(language);
    found.dedup();
    found.retain(|l| !l.is_empty() && l != "C" && l != "POSIX");
    found
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
fn screenshots(link: BusLink) -> ScreenshotSetup {
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
    .with_picker(Box::new(ShellColorPicker::new(link)))
}

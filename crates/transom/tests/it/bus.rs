//! A private session bus for the tests that need a real one. It listens in a scratch directory,
//! activates nothing, and is killed when the value drops; a test never touches the person's bus.

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

/// A `dbus-daemon` of its own.
pub struct PrivateBus {
    child: Child,
    /// The address a client connects to.
    pub address: String,
    _scratch: tempfile::TempDir,
}

impl PrivateBus {
    /// Start one. Without a `dbus-daemon` there is nothing to test against: the answer is `None`,
    /// said on stderr so the skip is visible, and a test never falls back to the real bus.
    pub fn start() -> Option<Self> {
        let scratch = tempfile::tempdir().ok()?;
        let config = scratch.path().join("bus.conf");
        let text = format!(
            "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\"\n \
             \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
             <busconfig>\n  <type>session</type>\n  <keep_umask/>\n  \
             <listen>unix:tmpdir={}</listen>\n  <auth>EXTERNAL</auth>\n  \
             <policy context=\"default\">\n    <allow send_destination=\"*\" eavesdrop=\"true\"/>\n    \
             <allow eavesdrop=\"true\"/>\n    <allow own=\"*\"/>\n  </policy>\n</busconfig>\n",
            scratch.path().display()
        );
        std::fs::write(&config, text).ok()?;
        let mut child = match Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                eprintln!("SKIPPED: no private bus, dbus-daemon did not start ({error})");
                return None;
            }
        };
        let mut address = String::new();
        let read = child
            .stdout
            .take()
            .map(|stdout| BufReader::new(stdout).read_line(&mut address));
        let address = address.trim().to_owned();
        if !matches!(read, Some(Ok(_))) || !address.starts_with("unix:") {
            let _ = child.kill();
            let _ = child.wait();
            eprintln!("SKIPPED: no private bus, dbus-daemon gave no address");
            return None;
        }
        Some(Self {
            child,
            address,
            _scratch: scratch,
        })
    }

    /// A connection builder for this bus.
    pub fn builder(&self) -> zbus::connection::Builder<'_> {
        zbus::connection::Builder::address(self.address.as_str())
            .unwrap_or_else(|error| panic!("the private bus address: {error}"))
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

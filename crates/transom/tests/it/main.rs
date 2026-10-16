//! transom's tests: one binary. Everything here runs on fakes; nothing opens a PipeWire daemon or
//! a compositor, and the one module that needs a bus (`shell_bus`) starts a private one and skips,
//! saying so, without a `dbus-daemon`. The jailed chain's checks live in `dev/accept/`.

mod bus;
mod capture;
mod chooser;
mod dev_callers;
mod fill;
mod guard;
mod inhibit;
mod names;
mod options;
mod peers;
mod pixels;
mod remote_desktop;
mod restore_file;
mod screenshot;
mod session;
mod settings;
mod shell_bus;
mod shell_check;
mod stream;

//! transom's tests: one binary. Everything here runs on fakes; nothing opens a bus, a PipeWire
//! daemon or a compositor. The jailed chain's checks live in `dev/accept/`.

mod capture;
mod chooser;
mod fill;
mod inhibit;
mod screenshot;
mod session;
mod settings;
mod stream;

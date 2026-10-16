//! A person's own rows (feature `debug`, which turns on peephole's `dev-rows`): the stand-ins a
//! checkout builds (a fake portal frontend, a fake shell), named in a file the person owns, are let
//! in as the programs they stand for, at `Proof::Dev`, below every real proof. A release build has
//! none of this: the feature, the environment variable and peephole's `dev-rows` are all absent.

use std::path::Path;

use peephole_bus::peephole::{LoadError, RealProc, TableFiles};
use peephole_bus::peephole_core::{Table, Uid};

/// `shipped`, with the developer rows in `path` added after its own (a row never takes a unit the
/// shipped table names). The file must be a plain file of this user's (or root's) that no one else
/// can write, and its rows must parse; otherwise the error says why and nothing is added.
pub fn with_dev_rows(shipped: Table, path: &Path) -> Result<Table, LoadError> {
    let person = Uid::new(rustix::process::getuid().as_raw());
    let rows = TableFiles::load_dev(&RealProc::system(), path, person)?;
    Ok(shipped.merge(rows))
}

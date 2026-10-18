//! Reading the options of a ScreenCast call and writing its results.
//!
//! These are the wire shapes: dictionaries of variants in, dictionaries of variants out. The
//! functions are pure so a table of cases covers them.

use std::collections::HashMap;

use transom_core::{
    CastSource, CursorMode, Extent, Keep, Multiple, Offer, PersistMode, PwNode, RestoreToken,
    SourceKinds,
};
use zbus::zvariant::{
    Array, Error as ValueError, OwnedValue, Signature, Structure, StructureBuilder, Value,
};

use super::Dict;
use super::session::Selection;

/// The vendor name in the `restore_data` this portal writes.
pub const VENDOR: &str = "quire";

/// The version of the private data in `restore_data`: the token as a string.
pub const DATA_VERSION: u32 = 1;

fn u32_of(options: &Dict, key: &str) -> Option<u32> {
    u32::try_from(options.get(key)?).ok()
}

/// What an app asked for in `SelectSources`. A missing or malformed option takes the portal's
/// default: displays, one source, the pointer hidden, nothing remembered.
pub fn selection(options: &Dict) -> Selection {
    let multiple = options
        .get("multiple")
        .and_then(|v| bool::try_from(v).ok())
        .map_or(Multiple::One, |many| {
            if many { Multiple::Many } else { Multiple::One }
        });
    Selection {
        offer: Offer {
            kinds: SourceKinds::from_wire(u32_of(options, "types").unwrap_or(1)),
            multiple,
            cursor_modes: cursor_modes(u32_of(options, "cursor_mode").unwrap_or(1)),
        },
        persist: u32_of(options, "persist_mode")
            .and_then(PersistMode::from_wire)
            .unwrap_or(PersistMode::DoNot),
        restore: restore_token(options),
    }
}

/// The modes in a portal bit set, hidden first. An empty set means hidden.
pub fn cursor_modes(bits: u32) -> Vec<CursorMode> {
    let modes: Vec<CursorMode> = [
        CursorMode::Hidden,
        CursorMode::Embedded,
        CursorMode::Metadata,
    ]
    .into_iter()
    .filter(|mode| bits & mode.to_wire() != 0)
    .collect();
    if modes.is_empty() {
        vec![CursorMode::Hidden]
    } else {
        modes
    }
}

/// The restore token in an app's `restore_data` `(suv)`, if it is one this portal wrote.
pub fn restore_token(options: &Dict) -> Option<RestoreToken> {
    let structure = <&Structure>::try_from(options.get("restore_data")?).ok()?;
    let [vendor, version, data] = structure.fields() else {
        return None;
    };
    let same_vendor = vendor.downcast_ref::<&str>().ok()? == VENDOR;
    let same_version = version.downcast_ref::<u32>().ok()? == DATA_VERSION;
    if !(same_vendor && same_version) {
        return None;
    }
    RestoreToken::parse(data.downcast_ref::<&str>().ok()?)
}

/// The `restore_data` to hand back for a token: `("quire", 1, <token>)`.
pub fn restore_data(token: &RestoreToken) -> Result<OwnedValue, ValueError> {
    let structure = StructureBuilder::new()
        .add_field(VENDOR)
        .add_field(DATA_VERSION)
        .append_field(Value::Value(Box::new(Value::new(
            token.expose().to_owned(),
        ))))
        .build()?;
    OwnedValue::try_from(structure)
}

/// One stream of a granted share, as the reply describes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StreamInfo {
    /// The node an app reads.
    pub node: PwNode,
    /// What it shows.
    pub source: CastSource,
    /// Its size in pixels.
    pub size: Extent,
}

/// The portal's `source_type` of a source: 1 for a display, 2 for a window.
pub fn source_type(source: &CastSource) -> u32 {
    match source {
        CastSource::Window(_) => 2,
        _ => 1,
    }
}

/// The reply's `streams`: `a(ua{sv})`, the node and its properties.
pub fn streams_value(streams: &[StreamInfo]) -> Result<OwnedValue, ValueError> {
    let signature = Signature::try_from("(ua{sv})")?;
    let mut array = Array::new(&signature);
    for stream in streams {
        let size = Structure::from((
            i32::try_from(stream.size.width).unwrap_or(i32::MAX),
            i32::try_from(stream.size.height).unwrap_or(i32::MAX),
        ));
        let mut properties: HashMap<String, OwnedValue> = HashMap::new();
        properties.insert(
            "source_type".to_owned(),
            OwnedValue::from(source_type(&stream.source)),
        );
        properties.insert("size".to_owned(), OwnedValue::try_from(size)?);
        let element = StructureBuilder::new()
            .add_field(stream.node.0)
            .add_field(properties)
            .build()?;
        array.append(Value::Structure(element))?;
    }
    OwnedValue::try_from(array)
}

/// The results of a granted Start: `streams`, and when a token is kept, `persist_mode` and
/// `restore_data`.
pub fn granted_results(
    streams: &[StreamInfo],
    kept: Option<(Keep, &RestoreToken)>,
) -> Result<Dict, ValueError> {
    let mut results = Dict::new();
    results.insert("streams".to_owned(), streams_value(streams)?);
    if let Some((keep, token)) = kept {
        results.insert("persist_mode".to_owned(), OwnedValue::from(keep.to_wire()));
        results.insert("restore_data".to_owned(), restore_data(token)?);
    }
    Ok(results)
}

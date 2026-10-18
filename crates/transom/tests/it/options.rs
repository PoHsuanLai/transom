use std::collections::HashMap;

use transom::screencast::Dict;
use transom::screencast::options::{
    StreamInfo, cursor_modes, granted_results, restore_data, restore_token, selection,
};
use transom_core::{
    CastSource, CursorMode, Extent, Keep, Multiple, PersistMode, PwNode, RestoreToken, SourceKinds,
};
use zbus::zvariant::{OwnedValue, Structure, StructureBuilder, Value};

fn dict(entries: Vec<(&str, OwnedValue)>) -> Dict {
    entries
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v))
        .collect::<HashMap<_, _>>()
}

#[test]
fn an_empty_options_dictionary_takes_the_portals_defaults() {
    let chosen = selection(&Dict::new());
    assert_eq!(chosen.offer.kinds, SourceKinds::Outputs);
    assert_eq!(chosen.offer.multiple, Multiple::One);
    assert_eq!(chosen.offer.cursor_modes, vec![CursorMode::Hidden]);
    assert_eq!(chosen.persist, PersistMode::DoNot);
    assert_eq!(chosen.restore, None);
}

#[test]
fn the_selection_reads_each_option() {
    let chosen = selection(&dict(vec![
        ("types", OwnedValue::from(3_u32)),
        ("multiple", OwnedValue::from(true)),
        ("cursor_mode", OwnedValue::from(6_u32)),
        ("persist_mode", OwnedValue::from(2_u32)),
    ]));
    assert_eq!(chosen.offer.kinds, SourceKinds::Both);
    assert_eq!(chosen.offer.multiple, Multiple::Many);
    assert_eq!(
        chosen.offer.cursor_modes,
        vec![CursorMode::Embedded, CursorMode::Metadata]
    );
    assert_eq!(chosen.persist, PersistMode::Keep(Keep::UntilRevoked));
}

#[test]
fn options_of_the_wrong_type_or_value_fall_back() {
    let chosen = selection(&dict(vec![
        ("types", OwnedValue::from(true)),
        ("persist_mode", OwnedValue::from(9_u32)),
        ("cursor_mode", OwnedValue::from(8_u32)),
    ]));
    assert_eq!(chosen.offer.kinds, SourceKinds::Outputs);
    assert_eq!(chosen.persist, PersistMode::DoNot);
    assert_eq!(chosen.offer.cursor_modes, vec![CursorMode::Hidden]);
}

#[test]
fn cursor_bits_become_modes_in_a_fixed_order() {
    for (bits, expected) in [
        (0, vec![CursorMode::Hidden]),
        (1, vec![CursorMode::Hidden]),
        (2, vec![CursorMode::Embedded]),
        (4, vec![CursorMode::Metadata]),
        (
            7,
            vec![
                CursorMode::Hidden,
                CursorMode::Embedded,
                CursorMode::Metadata,
            ],
        ),
    ] {
        assert_eq!(cursor_modes(bits), expected, "bits {bits}");
    }
}

#[test]
fn restore_data_round_trips_and_foreign_data_is_ignored() {
    let token = RestoreToken::mint([9; 16]);
    let ours = restore_data(&token).expect("restore data");
    let options = dict(vec![("restore_data", ours)]);
    assert_eq!(restore_token(&options), Some(token));

    let foreign = |vendor: &str, version: u32, data: &str| {
        let structure = StructureBuilder::new()
            .add_field(vendor)
            .add_field(version)
            .append_field(Value::Value(Box::new(Value::new(data.to_owned()))))
            .build()
            .expect("structure");
        dict(vec![(
            "restore_data",
            OwnedValue::try_from(structure).expect("owned"),
        )])
    };
    let good = RestoreToken::mint([1; 16]);
    let text = good.expose();
    assert_eq!(
        restore_token(&foreign("quire", 1, text)),
        Some(good.clone())
    );
    assert_eq!(restore_token(&foreign("GNOME", 1, text)), None);
    assert_eq!(restore_token(&foreign("quire", 2, text)), None);
    assert_eq!(restore_token(&foreign("quire", 1, "not a token")), None);
    let short = OwnedValue::try_from(Structure::from((1_u32, 2_u32))).expect("owned");
    assert_eq!(restore_token(&dict(vec![("restore_data", short)])), None);
}

#[test]
fn a_granted_reply_carries_streams_and_the_kept_token() {
    let streams = [
        StreamInfo {
            node: PwNode(41),
            source: CastSource::Output("A".into()),
            size: Extent {
                width: 1920,
                height: 1080,
            },
        },
        StreamInfo {
            node: PwNode(42),
            source: CastSource::Window("w".into()),
            size: Extent {
                width: 300,
                height: 200,
            },
        },
    ];
    let bare = granted_results(&streams, None).expect("results");
    assert!(bare.contains_key("streams"));
    assert!(!bare.contains_key("restore_data"));

    let token = RestoreToken::mint([3; 16]);
    let kept = granted_results(&streams, Some((Keep::UntilRevoked, &token))).expect("results");
    assert!(kept.contains_key("restore_data"));
    assert_eq!(
        u32::try_from(kept.get("persist_mode").expect("persist_mode")).ok(),
        Some(2)
    );
}

//! Track keys by name path (STD-125) over real bundles: keyed arrays hash the key, not the index the device gave.

use keyed_api::{Endpoint, FlowKey, Port, Proto};
use std::path::Path;
use wire_weaver::prelude::SerializeShrinkWrapOwned;
use ww_si::origins::DeviceMicros;
use ww_tape::{Segment, api_root_id, track_key};
use ww_tape_bundle::{NamePathError, key_in_bundle, name_path};

fn bundle(dir: &str, root: &str) -> ww_self::ApiBundleOwned {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(dir);
    wire_weaver_core::load(&dir, Some(root.into()), false).unwrap()
}

fn id(level: &ww_self::ApiLevelOwned, ident: &str) -> u32 {
    level.items.iter().find(|i| i.ident == ident).unwrap().id.0
}

fn flow_key(port: u16) -> Vec<u8> {
    let end = |port| Endpoint {
        addr: [10, 0, 0, 1],
        port: Port(port),
    };
    FlowKey {
        proto: Proto::Tcp,
        src: end(port),
        dst: end(80),
        first_seen: DeviceMicros::from_raw(1_000 + u64::from(port)),
    }
    .to_ww_bytes_owned()
    .unwrap()
}

/// Two runs give flows A and B the indices in another order: each flow's tracks keep their key.
#[test]
fn keyed_tracks_keep_their_key_whatever_the_index() {
    let b = bundle("tests/keyed_api", "Dissector");
    let flow = id(&b.root, "flow");
    let flow_level = b
        .traits
        .iter()
        .find_map(|t| match t {
            ww_self::ApiLevelLocationOwned::InLine { level, .. } if level.trait_name == "Flow" => {
                Some(level)
            }
            _ => None,
        })
        .unwrap();
    let packets = id(flow_level, "packets");
    let (a, b_key) = (flow_key(1234), flow_key(5678));
    let key = |index: u32, k: &[u8]| key_in_bundle(&b, &[flow, index, packets], &[(1, k)]).unwrap();

    let run1 = (key(0, &a), key(1, &b_key));
    let run2 = (key(1, &a), key(0, &b_key));
    assert_eq!(run1, run2);
    assert_ne!(run1.0, run1.1);
    assert_eq!(
        name_path(&b, &[flow, 7, packets], &[(1, &a)]).unwrap(),
        [
            Segment::Name("flow"),
            Segment::Key(&a),
            Segment::Name("packets")
        ]
    );
    assert_eq!(
        run1.0,
        track_key(
            api_root_id("keyed_api"),
            &[
                Segment::Name("flow"),
                Segment::Key(&a),
                Segment::Name("packets")
            ]
        )
    );
}

#[test]
fn keyed_index_needs_its_key() {
    let b = bundle("tests/keyed_api", "Dissector");
    let flow = id(&b.root, "flow");
    let clear = id(&b.root, "clear");
    let a = flow_key(1);
    assert!(matches!(
        name_path(&b, &[flow, 0, 1], &[]),
        Err(NamePathError::MissingKey { at: 1 })
    ));
    assert!(matches!(
        name_path(&b, &[clear], &[(0, &a)]),
        Err(NamePathError::StrayKey { at: 0 })
    ));
    assert!(matches!(
        name_path(&b, &[flow], &[]),
        Err(NamePathError::MissingIndex { at: 1 })
    ));
    assert!(matches!(
        name_path(&b, &[clear, 0], &[]),
        Err(NamePathError::TooLong { at: 1 })
    ));
    assert!(matches!(
        name_path(&b, &[99], &[]),
        Err(NamePathError::NoItem { at: 0, id: 99 })
    ));
}

/// The key companion's own stream, `flow_keys.keys`, is named by its fixed level.
#[test]
fn key_companion_items_are_named() {
    let b = bundle("tests/keyed_api", "Dissector");
    let flow_keys = id(&b.root, "flow_keys");
    let keys = ww_self::keys::KEYS;
    assert_eq!(
        name_path(&b, &[flow_keys, keys], &[]).unwrap(),
        [Segment::Name("flow_keys"), Segment::Name("keys")]
    );
    // `key[]` is a plain array: its index is hashed as it is
    let key = ww_self::keys::KEY;
    assert_eq!(
        name_path(&b, &[flow_keys, key, 3], &[]).unwrap(),
        [
            Segment::Name("flow_keys"),
            Segment::Name("key"),
            Segment::Index(3)
        ]
    );
}

/// The tap's keys name the streams of its `LinkTap` bundle.
#[test]
fn tap_keys_match_its_bundle() {
    let b = bundle("ww_tap", "LinkTap");
    use ww_tap::ids;
    for stream in [ids::SESSION, ids::EVENTS, ids::RX_FRAMES, ids::CLIENT_MAP] {
        assert_eq!(
            key_in_bundle(&b, &[ids::LINK, 2, stream], &[]).unwrap(),
            ww_tap::track_key(2, stream)
        );
    }
}

/// A resource renamed with `#[moved_from]` within its trait across a break keeps its key: `gain` on line 2 is
/// `amp_gain` on line 3, its identity name still `gain` (INT-17). The API root id ignores the crate's major.
#[test]
fn renamed_resource_keeps_its_key_across_a_break() {
    let w2 = bundle("tests/thermo_api", "wire_v2::Thermo");
    let w3 = bundle("tests/thermo_api", "wire_v3::Thermo");
    let on_w2 = key_in_bundle(&w2, &[id(&w2.root, "gain")], &[]).unwrap();
    let on_w3 = key_in_bundle(&w3, &[id(&w3.root, "amp_gain")], &[]).unwrap();
    assert_eq!(on_w2, on_w3);
    assert_eq!(
        on_w3,
        track_key(api_root_id("thermo_api"), &[Segment::Name("gain")])
    );
    // a resource that is not renamed keeps its key too, even if its id changed
    let mode = |b: &ww_self::ApiBundleOwned| key_in_bundle(b, &[id(&b.root, "mode")], &[]).unwrap();
    assert_eq!(mode(&w2), mode(&w3));
}

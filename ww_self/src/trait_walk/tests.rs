use shrink_wrap::prelude::*;
use ww_version::{FullVersionOwned, VersionOwned};

use super::*;
use crate::{ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevelLocationOwned, ApiLevelOwned};

fn item(id: u32, ident: &str, trait_idx: u32, multiplicity: Multiplicity) -> ApiItemOwned {
    ApiItemOwned {
        id: UNib32(id),
        kind: ApiItemKindOwned::Trait {
            trait_idx: UNib32(trait_idx),
            omitted: None,
        },
        multiplicity,
        since: None,
        ident: ident.to_owned(),
        docs: vec![],
        optional: None,
        meta: None,
        identity: None,
    }
}

fn level(
    crate_idx: u32,
    name: &str,
    compact: Option<u32>,
    items: Vec<ApiItemOwned>,
) -> ApiLevelOwned {
    ApiLevelOwned {
        docs: vec![],
        crate_idx: UNib32(crate_idx),
        trait_name: name.to_owned(),
        items,
        compact_id: compact.map(UNib32),
        meta: None,
    }
}

fn inline(level: ApiLevelOwned) -> ApiLevelLocationOwned {
    ApiLevelLocationOwned::InLine {
        crate_idx: level.crate_idx,
        level,
    }
}

fn bundle(root: ApiLevelOwned, traits: Vec<ApiLevelLocationOwned>) -> ApiBundleOwned {
    ApiBundleOwned {
        magic: crate::MAGIC,
        ww_self_version: crate::VERSION,
        root,
        types: vec![],
        traits,
        ext_crates: [
            ("hub_api", 1),
            ("player_api", 3),
            ("info_api", 2),
            ("gpio_api", 4),
        ]
        .iter()
        .map(|(id, minor)| FullVersionOwned::new((*id).to_owned(), VersionOwned::new(0, *minor, 1)))
        .collect(),
        root_omitted: None,
        wire: None,
        ext_wire: None,
    }
}

/// `Hub`: `players: [Player]` at 0, `info: Info` at 2, `player: Player` at 3 (compact id 77) with `info: Info` at 1
/// inside, `gpio: Gpio` at 4 (left out of the model).
fn hub() -> ApiBundleOwned {
    let flat = Multiplicity::Flat;
    bundle(
        level(
            0,
            "Hub",
            None,
            vec![
                item(
                    0,
                    "players",
                    0,
                    Multiplicity::Array {
                        index_type_idx: None,
                    },
                ),
                item(2, "info", 1, flat.clone()),
                item(3, "player", 0, flat.clone()),
                item(4, "gpio", 2, flat.clone()),
            ],
        ),
        vec![
            inline(level(1, "Player", Some(77), vec![item(1, "info", 1, flat)])),
            inline(level(2, "Info", None, vec![])),
            ApiLevelLocationOwned::SkippedFullVersion {
                crate_idx: UNib32(3),
                trait_name: "Gpio".to_owned(),
                signature: vec![],
            },
        ],
    )
}

fn full(crate_id: &'static str, minor: u32, trait_name: &'static str) -> WantedTrait<'static> {
    WantedTrait::Full {
        crate_version: FullVersion::new(crate_id, Version::new(0, minor, 0)),
        trait_name,
    }
}

/// The walk over `owned`'s bytes, checked against the owned walk the host's `MountedTraits` uses, and the path.
fn find(owned: &ApiBundleOwned, wanted: &WantedTrait<'_>) -> (TraitWalk, Vec<u32>) {
    let bytes = owned.to_ww_bytes_owned().expect("a bundle serializes");
    let bundle = ApiBundle::from_ww_bytes(&bytes).expect("a bundle decodes");
    let mut path = [0u32; TRAIT_WALK_DEPTH_MAX];
    let walk = bundle.find_trait(wanted, &mut path);
    let n = match walk {
        TraitWalk::Found(n) => n,
        _ => return (walk, vec![]),
    };
    let owned_path = owned.trait_path(|level, api| {
        let have = api.crate_version(level.crate_idx.0).expect("a crate");
        let version = Version::new(
            have.version.major.0,
            have.version.minor.0,
            have.version.patch.0,
        );
        let level_bytes = level.to_ww_bytes_owned().expect("a level serializes");
        let level = ApiLevel::from_ww_bytes(&level_bytes).expect("a level decodes");
        wanted.is_level(&level, &FullVersion::new(&have.crate_id, version))
    });
    if let Some(owned_path) = owned_path {
        assert_eq!(&path[..n], owned_path.as_slice(), "the owned walk's path");
    }
    (walk, path[..n].to_vec())
}

fn found(owned: &ApiBundleOwned, wanted: &WantedTrait<'_>) -> Option<Vec<u32>> {
    match find(owned, wanted) {
        (TraitWalk::Found(_), path) => Some(path),
        (TraitWalk::NotFound, _) => None,
        (TraitWalk::Malformed, _) => panic!("the model decodes"),
    }
}

#[test]
fn finds_traits_depth_first_never_under_an_array() {
    let hub = hub();
    assert_eq!(
        found(&hub, &full("hub_api", 1, "Hub")),
        Some(vec![]),
        "the root"
    );
    assert_eq!(
        found(&hub, &full("player_api", 3, "Player")),
        Some(vec![3]),
        "not the array at 0"
    );
    assert_eq!(
        found(&hub, &full("info_api", 2, "Info")),
        Some(vec![2]),
        "the first, depth first"
    );
    assert_eq!(
        found(&hub, &full("player_api", 4, "Player")),
        None,
        "another minor before 1.0"
    );
    assert_eq!(found(&hub, &full("player_api", 3, "Missing")), None);
    let compact = WantedTrait::Compact {
        id: 77,
        version: Version::new(0, 3, 9),
    };
    assert_eq!(found(&hub, &compact), Some(vec![3]));
    let other_minor = WantedTrait::Compact {
        id: 77,
        version: Version::new(0, 2, 0),
    };
    assert_eq!(found(&hub, &other_minor), None);
}

#[test]
fn nested_trait_below_the_first_match_of_another() {
    let mut hub = hub();
    // `info` at 2 gone: the Info inside `player` is the first
    hub.root.items.remove(1);
    assert_eq!(found(&hub, &full("info_api", 2, "Info")), Some(vec![3, 1]));
}

#[test]
fn a_left_out_trait_is_found_by_name_only() {
    let hub = hub();
    assert_eq!(found(&hub, &full("gpio_api", 4, "Gpio")), Some(vec![4]));
    let bytes = hub.to_ww_bytes_owned().expect("serializes");
    let bundle = ApiBundle::from_ww_bytes(&bytes).expect("decodes");
    let mut path = [0u32; 4];
    assert_eq!(
        bundle.find_trait(&full("gpio_api", 3, "Gpio"), &mut path),
        TraitWalk::NotFound
    );
}

#[test]
fn depth_is_limited_by_the_buffer() {
    let mut hub = hub();
    hub.root.items.remove(1);
    let bytes = hub.to_ww_bytes_owned().expect("serializes");
    let bundle = ApiBundle::from_ww_bytes(&bytes).expect("decodes");
    let info = full("info_api", 2, "Info");
    let mut one = [0u32; 1];
    assert_eq!(bundle.find_trait(&info, &mut one), TraitWalk::NotFound);
    let mut two = [0u32; 2];
    assert_eq!(bundle.find_trait(&info, &mut two), TraitWalk::Found(2));
    assert_eq!(two, [3, 1]);
    let mut none: [u32; 0] = [];
    assert_eq!(bundle.find_trait(&info, &mut none), TraitWalk::NotFound);
}

/// A trait that holds itself twice: the walk ends at its depth and item limits, never loops.
#[test]
fn a_model_that_refers_to_itself_ends() {
    let flat = Multiplicity::Flat;
    let looped = bundle(
        level(0, "Hub", None, vec![item(0, "a", 0, flat.clone())]),
        vec![inline(level(
            1,
            "Player",
            None,
            vec![item(0, "a", 0, flat.clone()), item(1, "b", 0, flat)],
        ))],
    );
    assert_eq!(found(&looped, &full("info_api", 2, "Info")), None);
}

#[test]
fn a_bad_index_is_malformed() {
    let flat = Multiplicity::Flat;
    let bad = bundle(level(0, "Hub", None, vec![item(0, "a", 9, flat)]), vec![]);
    assert_eq!(
        find(&bad, &full("info_api", 2, "Info")).0,
        TraitWalk::Malformed
    );
    let mut no_crate = hub();
    no_crate.root.crate_idx = UNib32(40);
    assert_eq!(
        find(&no_crate, &full("info_api", 2, "Info")).0,
        TraitWalk::Malformed
    );
}

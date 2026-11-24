use shrink_wrap::prelude::*;
use ww_version::{FullVersionOwned, VersionOwned};

use core::ops::ControlFlow;

use ww_version::Version;

use super::*;
use crate::{
    ApiBundleOwned, ApiItemKindOwned, ApiItemOwned, ApiLevel, ApiLevelLocationOwned, ApiLevelOwned,
};

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

/// The trait a global path asks for, as a firmware's table matches it.
enum Wanted {
    Full {
        crate_version: FullVersion<'static>,
        trait_name: &'static str,
    },
    Compact {
        id: u32,
        version: Version<'static>,
    },
}

impl Wanted {
    fn is(&self, at: &TraitAt<'_, '_>) -> bool {
        match self {
            Wanted::Full {
                crate_version,
                trait_name,
            } => {
                at.trait_name == *trait_name
                    && at.crate_version.is_protocol_compatible(crate_version)
            }
            Wanted::Compact { id, version } => {
                at.compact_id == Some(*id)
                    && at.crate_version.version.is_protocol_compatible(version)
            }
        }
    }
}

fn full(crate_id: &'static str, minor: u32, trait_name: &'static str) -> Wanted {
    Wanted::Full {
        crate_version: FullVersion::new(crate_id, Version::new(0, minor, 0)),
        trait_name,
    }
}

/// How a lookup by a walk ended.
#[derive(Debug, PartialEq, Eq)]
enum Look {
    Found(Vec<u32>),
    NotFound,
    Ended(TraitWalk),
}

/// The first trait the walk of `bundle` visits that is `wanted`, its path at most `depth` ids long.
fn look(bundle: &ApiBundle<'_>, wanted: &Wanted, depth: usize) -> Look {
    let mut path = [0u32; TRAIT_WALK_DEPTH_MAX];
    let mut found = None;
    let walk = bundle.visit_traits(&mut path[..depth], |at| {
        if wanted.is(at) {
            found = Some(at.path.to_vec());
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    });
    match (walk, found) {
        (TraitWalk::Stopped, Some(path)) => Look::Found(path),
        (TraitWalk::Done, None) => Look::NotFound,
        (walk, _) => Look::Ended(walk),
    }
}

/// The walk over `owned`'s bytes, checked against the owned walk the host's `MountedTraits` uses.
fn find(owned: &ApiBundleOwned, wanted: &Wanted) -> Look {
    let bytes = owned.to_ww_bytes_owned().expect("a bundle serializes");
    let bundle = ApiBundle::from_ww_bytes(&bytes).expect("a bundle decodes");
    let found = look(&bundle, wanted, TRAIT_WALK_DEPTH_MAX);
    if let Look::Found(path) = &found {
        let owned_path = owned.trait_path(|level, api| {
            let have = api.crate_version(level.crate_idx.0).expect("a crate");
            let version = Version::new(
                have.version.major.0,
                have.version.minor.0,
                have.version.patch.0,
            );
            wanted.is(&TraitAt {
                path: &[],
                crate_version: FullVersion::new(&have.crate_id, version),
                trait_name: &level.trait_name,
                compact_id: level.compact_id.map(|id| id.0),
            })
        });
        // the owned walk finds no left out trait: its client puts them back first
        if let Some(owned_path) = owned_path {
            assert_eq!(*path, owned_path, "the owned walk's path");
        }
    }
    found
}

fn found(owned: &ApiBundleOwned, wanted: &Wanted) -> Option<Vec<u32>> {
    match find(owned, wanted) {
        Look::Found(path) => Some(path),
        Look::NotFound => None,
        Look::Ended(walk) => panic!("the walk ended early: {walk:?}"),
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
    let compact = Wanted::Compact {
        id: 77,
        version: Version::new(0, 3, 9),
    };
    assert_eq!(found(&hub, &compact), Some(vec![3]));
    let other_minor = Wanted::Compact {
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
    assert_eq!(found(&hub, &full("gpio_api", 3, "Gpio")), None);
}

#[test]
fn depth_is_limited_by_the_buffer() {
    let mut hub = hub();
    hub.root.items.remove(1);
    let bytes = hub.to_ww_bytes_owned().expect("serializes");
    let bundle = ApiBundle::from_ww_bytes(&bytes).expect("decodes");
    let info = full("info_api", 2, "Info");
    // `player` at 3 holds a trait one level below: a walk one level deep says so
    assert_eq!(look(&bundle, &info, 1), Look::Ended(TraitWalk::Limit));
    assert_eq!(look(&bundle, &info, 2), Look::Found(vec![3, 1]));
    assert_eq!(look(&bundle, &info, 0), Look::Ended(TraitWalk::Limit));
    let mut none: [u32; 0] = [];
    let mut visited = 0;
    let walk = bundle.visit_traits(&mut none, |_| {
        visited += 1;
        ControlFlow::Continue(())
    });
    assert_eq!((walk, visited), (TraitWalk::Limit, 1), "the root only");
}

/// Every trait in walk order, the first visit of each is the one a global path reaches.
#[test]
fn visits_every_trait_depth_first() {
    let bytes = hub().to_ww_bytes_owned().expect("serializes");
    let bundle = ApiBundle::from_ww_bytes(&bytes).expect("decodes");
    let mut path = [0u32; 4];
    let mut visited = vec![];
    let walk = bundle.visit_traits(&mut path, |at| {
        visited.push((at.path.to_vec(), at.trait_name, at.compact_id));
        ControlFlow::Continue(())
    });
    assert_eq!(walk, TraitWalk::Done);
    assert_eq!(
        visited,
        [
            (vec![], "Hub", None),
            (vec![2], "Info", None),
            (vec![3], "Player", Some(77)),
            (vec![3, 1], "Info", None),
            (vec![4], "Gpio", None),
        ]
    );
}

/// A trait that holds itself twice: the walk ends at its depth and item limits, never loops, and says so.
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
    assert_eq!(
        find(&looped, &full("info_api", 2, "Info")),
        Look::Ended(TraitWalk::Limit)
    );
}

#[test]
fn a_bad_index_is_malformed() {
    let flat = Multiplicity::Flat;
    let bad = bundle(level(0, "Hub", None, vec![item(0, "a", 9, flat)]), vec![]);
    assert_eq!(
        find(&bad, &full("info_api", 2, "Info")),
        Look::Ended(TraitWalk::Malformed)
    );
    let mut no_crate = hub();
    no_crate.root.crate_idx = UNib32(40);
    assert_eq!(
        find(&no_crate, &full("info_api", 2, "Info")),
        Look::Ended(TraitWalk::Malformed)
    );
}

/// A model built in memory (`RefVec::Slice`) walks as its bytes do.
#[test]
fn a_model_in_memory_walks_as_its_bytes() {
    let crates = [
        FullVersion::new("hub_api", Version::new(0, 1, 1)),
        FullVersion::new("player_api", Version::new(0, 3, 1)),
    ];
    let empty = RefVec::Slice { slice: &[] };
    let item = |id, ident, multiplicity| ApiItem {
        id: UNib32(id),
        kind: ApiItemKind::Trait {
            trait_idx: UNib32(0),
            omitted: None,
        },
        multiplicity,
        since: None,
        ident,
        docs: empty,
        optional: None,
        meta: None,
        identity: None,
    };
    let items = [
        item(
            0,
            "players",
            Multiplicity::Array {
                index_type_idx: None,
            },
        ),
        item(3, "player", Multiplicity::Flat),
    ];
    let traits = [ApiLevelLocation::InLine {
        level: ApiLevel {
            docs: empty,
            crate_idx: UNib32(1),
            trait_name: "Player",
            items: RefVec::Slice { slice: &[] },
            compact_id: Some(UNib32(77)),
            meta: None,
        },
        crate_idx: UNib32(1),
    }];
    let bundle = ApiBundle {
        magic: crate::MAGIC,
        ww_self_version: crate::VERSION,
        root: ApiLevel {
            docs: empty,
            crate_idx: UNib32(0),
            trait_name: "Hub",
            items: RefVec::Slice { slice: &items },
            compact_id: None,
            meta: None,
        },
        types: RefVec::Slice { slice: &[] },
        traits: RefVec::Slice { slice: &traits },
        ext_crates: RefVec::Slice { slice: &crates },
        root_omitted: None,
        wire: None,
        ext_wire: None,
    };
    let player = full("player_api", 3, "Player");
    assert_eq!(look(&bundle, &player, 4), Look::Found(vec![3]));
    let mut buf = [0u8; 256];
    let bytes = bundle.to_ww_bytes(&mut buf).expect("serializes");
    let decoded = ApiBundle::from_ww_bytes(bytes).expect("decodes");
    assert_eq!(look(&decoded, &player, 4), Look::Found(vec![3]));
}

//! Requests against `shared_handlers_api`, each with whether `is_shared` takes it, for both test servers: their
//! receiver models make the same items shared.
//!
//! Resource ids by declaration order: root `add` 0, `add_later` 1, `bump` 2, `status` 3, `counter` 4, `events` 5,
//! `sensors` 6, `motor` 7, `channel[]` 8, `flow[]` 9, `flow_keys` 10, `info` 11; `Sensors`: `temp` 0,
//! `calibrate` 1; `Motor`: `speed` 0; `Channel`: `level` 0, `scaled` 1; `Flow`: `packets` 0; the key companion:
//! `index_of` 0, `key[]` 1, `keys` 2; `GlobalDemo`: `CAPABILITIES` 0, `name` 1.

use global_fixture_api::{GLOBAL_DEMO_COMPACT_GID, GLOBAL_DEMO_FULL_GID};
use shared_handlers_api::FlowKey;
use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::UVlq32Backfill;
use wire_weaver::shrink_wrap::tail_bytes::TailBytes;
use ww_client_server::{
    MultiArgs, MultiIndex, PathKind, Priority, Request, RequestKind, ResourceIndex, resource_path,
};

/// Arguments of `add(a, b)` and `add_later(a, b)`.
#[derive_shrink_wrap(borrowed)]
pub struct AddArgs {
    pub a: u32,
    pub b: u32,
}

/// Arguments of `channel[i].scaled(by)`.
#[derive_shrink_wrap(borrowed)]
pub struct ScaledArgs {
    pub by: u32,
}

/// Arguments of `sensors.calibrate(offset)`.
#[derive_shrink_wrap(borrowed)]
struct CalibrateArgs {
    offset: i32,
}

/// Arguments of `flow_keys.index_of(key)`.
#[derive_shrink_wrap(borrowed)]
struct IndexOfArgs {
    key: FlowKey,
}

/// One request and whether it goes to the shared entry.
pub struct Case {
    pub name: &'static str,
    pub bytes: Vec<u8>,
    pub shared: bool,
}

/// Request bytes of `kind` on `path_kind` with `seq`.
pub fn request(seq: u32, path_kind: PathKind<'_>, kind: RequestKind<'_>) -> Vec<u8> {
    let req = Request {
        seq: UVlq32Backfill(seq),
        priority: Priority::NORMAL.to_wire(),
        context: None,
        path_kind,
        kind,
    };
    let mut buf = [0u8; 256];
    req.to_ww_bytes(&mut buf)
        .expect("a test request fits")
        .to_vec()
}

fn absolute(path: &[ResourceIndex]) -> PathKind<'_> {
    PathKind::Absolute {
        path: RefVec::Slice { slice: path },
    }
}

fn ser<T: SerializeShrinkWrap>(value: &T) -> Vec<u8> {
    let mut buf = [0u8; 64];
    value
        .to_ww_bytes(&mut buf)
        .expect("test arguments fit")
        .to_vec()
}

/// A call of `path` with arguments `args`.
pub fn call(seq: u32, path: &[ResourceIndex], args: &[u8]) -> Vec<u8> {
    request(
        seq,
        absolute(path),
        RequestKind::Call {
            args: TailBytes(args),
        },
    )
}

/// A read of `path`.
pub fn read(seq: u32, path: &[ResourceIndex]) -> Vec<u8> {
    request(seq, absolute(path), RequestKind::Read)
}

fn write(path: &[ResourceIndex], data: &[u8]) -> Vec<u8> {
    request(
        1,
        absolute(path),
        RequestKind::Write {
            data: TailBytes(data),
        },
    )
}

fn open(path: &[ResourceIndex]) -> Vec<u8> {
    request(
        1,
        absolute(path),
        RequestKind::StreamOpen {
            resume: None,
            max_age: None,
            raw: false,
            args: TailBytes(&[]),
        },
    )
}

fn global_full(kind: RequestKind<'_>) -> Vec<u8> {
    request(
        1,
        PathKind::GlobalFull {
            gid: GLOBAL_DEMO_FULL_GID,
            trait_name: "GlobalDemo",
            path_from_trait: RefVec::Slice {
                slice: &resource_path([1]),
            },
        },
        kind,
    )
}

/// `add(2, 3)` with `seq`.
pub fn add(seq: u32) -> Vec<u8> {
    call(seq, &resource_path([0]), &ser(&AddArgs { a: 2, b: 3 }))
}

/// `add_later(2, 3)`, answered later.
pub fn add_later() -> Vec<u8> {
    call(1, &resource_path([1]), &ser(&AddArgs { a: 2, b: 3 }))
}

/// `channel[1].scaled(by)`.
pub fn scaled(by: u32) -> Vec<u8> {
    call(1, &resource_path([8, 1, 1]), &ser(&ScaledArgs { by }))
}

/// Every case, in the order the tests run them on fresh servers.
pub fn cases() -> Vec<Case> {
    let mut cases = item_cases();
    cases.extend(global_cases());
    cases.extend(multi_cases());
    cases.extend(vec![
        case("no such item", read(1, &resource_path([99])), false),
        case(
            "introspect",
            request(1, absolute(&[]), RequestKind::Introspect),
            false,
        ),
    ]);
    cases
}

fn case(name: &'static str, bytes: Vec<u8>, shared: bool) -> Case {
    Case {
        name,
        bytes,
        shared,
    }
}

/// `MaxLen<&str, 32>` "hi", the data of a write to `info.name`.
fn name_data() -> Vec<u8> {
    let mut buf = [0u8; 64];
    wire_weaver::shrink_wrap::MaxLen::<&str, 32>::new("hi")
        .expect("short")
        .to_ww_bytes(&mut buf)
        .expect("fits")
        .to_vec()
}

/// Single requests on items of the root, its levels and arrays.
fn item_cases() -> Vec<Case> {
    let key_args = ser(&IndexOfArgs { key: FlowKey(20) });
    let name_data = name_data();
    vec![
        case("add", add(1), true),
        case("add with seq 0", add(0), false),
        case("add_later (deferred)", add_later(), true),
        case("bump", call(1, &resource_path([2]), &[]), false),
        case("status", read(1, &resource_path([3])), true),
        case("counter read", read(1, &resource_path([4])), true),
        case(
            "counter write",
            write(&resource_path([4]), &ser(&9u32)),
            false,
        ),
        case("events open", open(&resource_path([5])), false),
        case("sensors.temp", read(1, &resource_path([6, 0])), true),
        case(
            "sensors.calibrate",
            call(
                1,
                &resource_path([6, 1]),
                &ser(&CalibrateArgs { offset: 1 }),
            ),
            false,
        ),
        case("motor.speed read", read(1, &resource_path([7, 0])), false),
        case("channel indices", read(1, &resource_path([8])), true),
        case("channel[1].level", read(1, &resource_path([8, 1, 0])), true),
        case("channel[1].scaled", scaled(3), true),
        case(
            "channel[9] bad index",
            read(1, &resource_path([8, 9, 0])),
            true,
        ),
        case("flow[1].packets", read(1, &resource_path([9, 1, 0])), true),
        case("flow indices", read(1, &resource_path([9])), true),
        case(
            "flow_keys.index_of",
            call(1, &resource_path([10, 0]), &key_args),
            true,
        ),
        case(
            "flow_keys.key[1]",
            read(1, &resource_path([10, 1, 1])),
            true,
        ),
        case(
            "flow_keys.key indices",
            read(1, &resource_path([10, 1])),
            true,
        ),
        case("flow_keys.keys open", open(&resource_path([10, 2])), false),
        case("info.name", read(1, &resource_path([11, 1])), true),
        case(
            "info.name write",
            write(&resource_path([11, 1]), &name_data),
            false,
        ),
        case("info.CAPABILITIES", read(1, &resource_path([11, 0])), false),
    ]
}

/// Requests through a global trait path, full and compact.
fn global_cases() -> Vec<Case> {
    let name_data = name_data();
    vec![
        case(
            "global full info.name",
            global_full(RequestKind::Read),
            true,
        ),
        case(
            "global compact info.name",
            request(
                1,
                PathKind::GlobalCompact {
                    gid: GLOBAL_DEMO_COMPACT_GID.expect("GlobalDemo has a ww_global id"),
                    path_from_trait: RefVec::Slice {
                        slice: &resource_path([1]),
                    },
                },
                RequestKind::Read,
            ),
            true,
        ),
        case(
            "global full info.name write",
            global_full(RequestKind::Write {
                data: TailBytes(&name_data),
            }),
            false,
        ),
    ]
}

/// Multi requests over arrays and keyed arrays.
fn multi_cases() -> Vec<Case> {
    let level_multi = MultiIndex::List(RefVec::Slice {
        slice: &resource_path([0, 1, 2]),
    });
    let keys_multi = MultiIndex::List(RefVec::Slice {
        slice: &resource_path([0, 1]),
    });
    let scaled_args = ser(&ScaledArgs { by: 3 });
    vec![
        case(
            "multi read channel[].level",
            request(
                1,
                absolute(&resource_path([8])),
                RequestKind::MultiRead {
                    multi_idx: level_multi.clone(),
                    in_each_array_id: Some(ResourceIndex::new(UNib32(0))),
                },
            ),
            true,
        ),
        case(
            "multi call channel[].scaled",
            request(
                1,
                absolute(&resource_path([8])),
                RequestKind::MultiCall {
                    multi_idx: level_multi.clone(),
                    in_each_array_id: Some(ResourceIndex::new(UNib32(1))),
                    multi_args: MultiArgs::Same(RefVec::Slice {
                        slice: &scaled_args,
                    }),
                },
            ),
            true,
        ),
        case(
            "multi read flow_keys.key[]",
            request(
                1,
                absolute(&resource_path([10, 1])),
                RequestKind::MultiRead {
                    multi_idx: keys_multi,
                    in_each_array_id: None,
                },
            ),
            true,
        ),
        case(
            "multi read of a method",
            request(
                1,
                absolute(&resource_path([8])),
                RequestKind::MultiRead {
                    multi_idx: level_multi.clone(),
                    in_each_array_id: Some(ResourceIndex::new(UNib32(1))),
                },
            ),
            false,
        ),
        case(
            "multi write channel[].level",
            request(
                1,
                absolute(&resource_path([8])),
                RequestKind::MultiWrite {
                    multi_idx: level_multi,
                    in_each_array_id: Some(ResourceIndex::new(UNib32(0))),
                    multi_data: MultiArgs::Same(RefVec::Slice {
                        slice: &scaled_args,
                    }),
                },
            ),
            false,
        ),
    ]
}

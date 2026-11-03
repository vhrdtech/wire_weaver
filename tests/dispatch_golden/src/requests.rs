//! Builds the request bytes of every corpus case (TEST-31): a small Rust function per case, analogous to
//! `tests/conformance`'s `derive.rs` building each case's value. Keeping these as code, not hand-written hex in
//! `corpus.json`, is what lets `DISPATCH_GOLDEN=update` fill the corpus, and is the other half of what's frozen:
//! a changed `request` means the test itself changed, not the dispatcher.
//!
//! Resource indices, by declaration order in `dispatch_golden_api` (0-based, as every nested level starts over):
//! root: no_args 0, with_args 1, absent 2, deferred 3, answer_deferred 4, plain 5, guarded 6, LIMIT 7, samples 8,
//! commands 9, channel[] 10, info 11; `Channel`: pulse 0, level 1; `ww_user_info::UserInfo`: CAPABILITIES 0,
//! name 1, notes 2.

use dispatch_golden_api::CHANNEL_FULL_GID;
use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::UVlq32Backfill;
use wire_weaver::shrink_wrap::tail_bytes::TailBytes;
use ww_client_server::{MultiArgs, MultiIndex, PathKind, Priority, Request, RequestKind};
use ww_user_info::{USER_INFO_COMPACT_GID, USER_INFO_FULL_GID};

/// Arguments of `with_args(a, b)`, in declaration order: a method's arguments are a struct of its own.
#[derive_shrink_wrap(borrowed)]
struct WithArgsArgs {
    a: u8,
    b: u16,
}

fn request(seq: u32, path_kind: PathKind<'_>, kind: RequestKind<'_>) -> Vec<u8> {
    let req = Request {
        seq: UVlq32Backfill(seq),
        priority: Priority::NORMAL.nibble(),
        path_kind,
        kind,
    };
    let mut buf = [0u8; 256];
    req.to_ww_bytes(&mut buf).unwrap().to_vec()
}

fn absolute(path: &[UNib32]) -> PathKind<'_> {
    PathKind::Absolute {
        path: RefVec::Slice { slice: path },
    }
}

fn call(seq: u32, path: &[UNib32], args: &[u8]) -> Vec<u8> {
    request(
        seq,
        absolute(path),
        RequestKind::Call {
            args: TailBytes(args),
        },
    )
}

fn read(seq: u32, path: &[UNib32]) -> Vec<u8> {
    request(seq, absolute(path), RequestKind::Read)
}

fn write(seq: u32, path: &[UNib32], data: &[u8]) -> Vec<u8> {
    request(
        seq,
        absolute(path),
        RequestKind::Write {
            data: TailBytes(data),
        },
    )
}

fn with_args_bytes(a: u8, b: u16) -> Vec<u8> {
    let mut buf = [0u8; 16];
    WithArgsArgs { a, b }
        .to_ww_bytes(&mut buf)
        .unwrap()
        .to_vec()
}

fn max_len_str_bytes<const N: usize>(s: &str) -> Vec<u8> {
    let value: MaxLen<&str, N> = MaxLen::new(s).unwrap();
    let mut buf = [0u8; 64];
    value.to_ww_bytes(&mut buf).unwrap().to_vec()
}

/// The request bytes of `name`'s `step`-th step (0-based), matching [crate::corpus::Case::steps]' order.
pub fn build(name: &str, step: usize) -> Vec<u8> {
    match (name, step) {
        ("call.no_args", 0) => call(1, &[UNib32(0)], &[]),
        ("call.with_args", 0) => call(1, &[UNib32(1)], &with_args_bytes(5, 300)),
        ("call.absent", 0) => call(1, &[UNib32(2)], &[]),
        ("multi.deferred_then_answer", 0) => call(7, &[UNib32(3)], &[]),
        ("multi.deferred_then_answer", 1) => call(8, &[UNib32(4)], &[0x37]),
        ("property.read", 0) => read(1, &[UNib32(5)]),
        ("property.write", 0) => write(1, &[UNib32(5)], &[7]),
        ("property.write_user_error", 0) => write(1, &[UNib32(6)], &[200]),
        ("const.read", 0) => read(1, &[UNib32(7)]),
        ("stream.open", 0) => request(
            1,
            absolute(&[UNib32(8)]),
            RequestKind::StreamOpen {
                resume: None,
                max_age_us: None,
                codecs: None,
                args: TailBytes(&[]),
            },
        ),
        ("sink.write", 0) => write(1, &[UNib32(9)], &[42]),
        ("array.call", 0) => call(1, &[UNib32(10), UNib32(0), UNib32(0)], &[]),
        ("array.property_write", 0) => write(1, &[UNib32(10), UNib32(1), UNib32(1)], &[5]),
        ("array.bad_index", 0) => call(1, &[UNib32(10), UNib32(99), UNib32(0)], &[]),
        ("global.full_read_name", 0) => request(
            1,
            PathKind::GlobalFull {
                gid: USER_INFO_FULL_GID,
                trait_name: "UserInfo",
                path_from_trait: RefVec::Slice {
                    slice: &[UNib32(1)],
                },
            },
            RequestKind::Read,
        ),
        ("global.compact_write_name", 0) => request(
            1,
            PathKind::GlobalCompact {
                gid: USER_INFO_COMPACT_GID.expect("ww_user_info::UserInfo has a ww_global id"),
                path_from_trait: RefVec::Slice {
                    slice: &[UNib32(1)],
                },
            },
            RequestKind::Write {
                data: TailBytes(&max_len_str_bytes::<32>("hi")),
            },
        ),
        ("global.compact_read_const", 0) => request(
            1,
            PathKind::GlobalCompact {
                gid: USER_INFO_COMPACT_GID.expect("ww_user_info::UserInfo has a ww_global id"),
                path_from_trait: RefVec::Slice {
                    slice: &[UNib32(0)],
                },
            },
            RequestKind::Read,
        ),
        ("global.full_incompatible_version", 0) => {
            let mut gid = USER_INFO_FULL_GID;
            gid.version.major = UNib32(gid.version.major.0 + 1);
            request(
                1,
                PathKind::GlobalFull {
                    gid,
                    trait_name: "UserInfo",
                    path_from_trait: RefVec::Slice {
                        slice: &[UNib32(1)],
                    },
                },
                RequestKind::Read,
            )
        }
        ("global.missing_trait", 0) => request(
            1,
            PathKind::GlobalFull {
                gid: CHANNEL_FULL_GID,
                trait_name: "Channel",
                path_from_trait: RefVec::Slice {
                    slice: &[UNib32(0)],
                },
            },
            RequestKind::Call {
                args: TailBytes(&[]),
            },
        ),
        ("error.bad_path", 0) => call(1, &[UNib32(999)], &[]),
        ("error.too_deep_path", 0) => call(1, &[UNib32(0), UNib32(0)], &[]),
        ("error.wrong_kind_call_on_property", 0) => call(1, &[UNib32(5)], &[]),
        ("error.wrong_kind_write_on_const", 0) => write(1, &[UNib32(7)], &[0, 0]),
        ("error.seq_zero_read_property", 0) => read(0, &[UNib32(5)]),
        ("error.seq_zero_call_no_reply", 0) => call(0, &[UNib32(0)], &[]),
        ("multi.multi_call", 0) => request(
            1,
            absolute(&[UNib32(10)]),
            RequestKind::MultiCall {
                multi_idx: MultiIndex::List(RefVec::Slice {
                    slice: &[UNib32(0), UNib32(1)],
                }),
                in_each_array_id: Some(UNib32(0)),
                multi_args: MultiArgs::Same(RefVec::Slice { slice: &[] }),
            },
        ),
        ("multi.multi_read", 0) => request(
            1,
            absolute(&[UNib32(10)]),
            RequestKind::MultiRead {
                multi_idx: MultiIndex::List(RefVec::Slice {
                    slice: &[UNib32(0), UNib32(1)],
                }),
                in_each_array_id: Some(UNib32(1)),
            },
        ),
        _ => panic!("no request builder for case '{name}' step {step}"),
    }
}

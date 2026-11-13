//! The requests of the dispatch budget (TEST-30), shared by `qemu-dispatch-requests` (instructions under QEMU) and
//! `usb_stm32h725ig`'s `stm32h725ig-dispatch-cycles` (cycles on silicon): one request of each kind to a server of
//! `flash_budget_api::BaseOnly`, as bytes, and the FNV hash their replies are compared by.

use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::UVlq32Backfill;
use wire_weaver::shrink_wrap::tail_bytes::TailBytes;
use ww_client_server::{PathKind, Priority, Request, RequestKind, StreamSideband};

/// Indices in `BaseOnly`: `base` 0, and in `Base`: c0 0, c1 1, a0 2, a1 3, p0 4, p1 5, s0 6, s1 7.
pub const CASES: [(&str, u32); 8] = [
    ("call, no arguments or output", 0),
    ("call, an argument and an output", 2),
    ("property read", 4),
    ("property write", 4),
    ("stream open", 6),
    ("stream close", 6),
    ("error: bad path", 9),
    ("error: kind not taken", 0),
];

/// The request bytes of case `i`, into `buf`.
pub fn request(i: usize, buf: &mut [u8]) -> &[u8] {
    let mut code = [0u8; 8];
    let code = flash_budget_api::Code(0x1234)
        .to_ww_bytes(&mut code)
        .unwrap();
    let kind = match i {
        0 | 6 => RequestKind::Call {
            args: TailBytes(&[]),
        },
        1 => RequestKind::Call {
            args: TailBytes(code),
        },
        2 | 7 => RequestKind::Read,
        3 => RequestKind::Write {
            data: TailBytes(code),
        },
        4 => RequestKind::StreamOpen {
            resume: None,
            max_age: None,
            codecs: None,
            args: TailBytes(&[]),
        },
        _ => RequestKind::StreamSideband {
            sideband: StreamSideband::Close,
        },
    };
    let path = ww_client_server::resource_path([0, CASES[i].1]);
    let req = Request {
        seq: UVlq32Backfill(7),
        priority: Priority::NORMAL.nibble(),
        path_kind: PathKind::absolute(&path),
        kind,
    };
    req.to_ww_bytes(buf).unwrap()
}

/// FNV-1a of a reply, to compare replies across builds and targets.
pub fn fnv(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c_9dc5u32, |h, &b| {
        (h ^ b as u32).wrapping_mul(0x0100_0193)
    })
}

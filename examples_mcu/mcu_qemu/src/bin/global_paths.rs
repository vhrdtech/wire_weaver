//! Cost of global trait paths on the device (API-20): a server of `tests/global_paths_api` (`alpha`, a local
//! fixture trait, at index 0 and `ww_timebase::Clock` at index 1) answers `Clock::now` through an absolute path, a
//! `GlobalFull` path (crate name, version and trait name), a `GlobalCompact` path (the trait's `ww_global` id) and
//! a compact path with an id it doesn't have (`TraitNotFound`). Each call is bracketed by `mark_start` / `mark_end`
//! for `qemu_icount`, the first bracket is empty (calibration):
//!
//! ```text
//! cargo build --release --bin qemu-global-paths
//! cd ../qemu_icount && cargo run --release -- cortex-m3 lm3s6965evb ../mcu_qemu/target/thumbv7m-none-eabi/release/qemu-global-paths
//! ```
//!
//! R10 moved this bench off `tests/dfu_api` (`ww_dfu`, a stdlib API crate) onto a framework-only fixture; `clock`
//! stays `ww_timebase::Clock` rather than a local trait because the `GlobalCompact` match case needs a trait with a
//! real `ww_global` id (24), which a non-stdlib trait doesn't get until SRV-61. `ww_timebase` is itself framework
//! (R5), not a standard API, so this keeps the bench off anything the `ww_stdlib` cut moves away.
//!
//! Instruction counts (the table this doc comment used to carry, from the `ww_dfu`-based fixture, 20 Oct 2026) need
//! remeasuring with `qemu_icount` on a PC that has `qemu-system-arm` (none available here). The firmware itself
//! shrank with the smaller fixture (`just zero-cost`, `examples_mcu/mcu_qemu` `streams` feature, this trait shape
//! against the 5-trait `DfuDevice` one): `fw-z` .text 19708 -> 16172 B (-3536), .rodata 2020 -> 1960 B (-60);
//! `fw-3` .text 28716 -> 23888 B (-4828), .rodata 1720 -> 1660 B (-60).
#![no_main]
#![no_std]

extern crate panic_semihosting;

use core::fmt::Write;
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hio};
use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::tail_bytes::TailBytes;
use wire_weaver::ww_version::{CompactVersion, FullVersion, GlobalTypeId, Version};
use ww_client_server::{PathKind, Request, RequestKind};

#[inline(never)]
fn mark_start() {
    unsafe { core::arch::asm!("nop", options(nostack, preserves_flags)) }
}

#[inline(never)]
fn mark_end() {
    unsafe { core::arch::asm!("nop", "nop", options(nostack, preserves_flags)) }
}

/// A `Call` request with seq 1, as it goes on the wire (seq in one byte).
fn request<'b>(path_kind: PathKind<'_>, buf: &'b mut [u8]) -> &'b [u8] {
    let request = Request {
        seq: UVlq32Backfill(1),
        priority: ww_client_server::Priority::NORMAL.to_wire(),
        reserved: false,
        path_kind,
        kind: RequestKind::Call {
            args: TailBytes(&[]),
        },
    };
    let len = request.to_ww_bytes(buf).unwrap().len();
    Request::set_seq(&mut buf[..len], 1).unwrap()
}

#[entry]
fn main() -> ! {
    let mut stdout = hio::hstdout().unwrap();
    let clock_now = ww_client_server::resource_path([0]);
    let absolute = ww_client_server::resource_path([1, 0]);
    let clock = ww_timebase::CLOCK_COMPACT_GID.unwrap();
    let (mut b0, mut b1, mut b2, mut b3) = ([0u8; 64], [0u8; 64], [0u8; 64], [0u8; 64]);
    let requests = [
        request(PathKind::absolute(&absolute), &mut b0),
        request(
            PathKind::GlobalFull {
                gid: FullVersion::new("ww_timebase", Version::new(0, 1, 0)),
                trait_name: "Clock",
                path_from_trait: RefVec::Slice { slice: &clock_now },
            },
            &mut b1,
        ),
        request(
            PathKind::GlobalCompact {
                gid: clock,
                path_from_trait: RefVec::Slice { slice: &clock_now },
            },
            &mut b2,
        ),
        request(
            PathKind::GlobalCompact {
                gid: CompactVersion::new(GlobalTypeId::new(4000), 0, 1, 0),
                path_from_trait: RefVec::Slice { slice: &clock_now },
            },
            &mut b3,
        ),
    ];

    let mut scratch = [0u8; 128];
    let mut event_scratch = [0u8; 128];
    let mut server = ServerState {};
    let mut out = EventWriter::new(DummyTx, &mut event_scratch);
    mark_start();
    mark_end();
    for request in requests {
        let request = core::hint::black_box(request);
        mark_start();
        let r = server.process_request_bytes(request, &mut scratch, &mut out, ());
        mark_end();
        writeln!(stdout, "{} B request -> {:02x?}", request.len(), r).unwrap();
    }

    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}

struct DummyTx;
impl wire_weaver::BlockingMessageSink for DummyTx {
    fn send(&mut self, _message: &[u8]) -> Result<(), ()> {
        Ok(())
    }
}

pub struct ServerState {}

impl ServerState {
    // ---- global_paths_api::PathBenchDevice at root ----

    // ---- global_paths_api::Alpha at alpha ----

    fn alpha_ping(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<bool> {
        Unimplemented.into()
    }

    // ---- ww_timebase::Clock at clock ----

    /// Device time now, full width.
    fn clock_now(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> RpcResult<ww_timebase::Second> {
        Unimplemented.into()
    }

    /// Static description of this clock.
    fn clock_info(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> RpcResult<ww_timebase::ClockInfo<'_>> {
        Unimplemented.into()
    }

    /// Opens the stream (a new session): `Ok` / `Ready` starts it.
    fn clock_time_map(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
        RpcResult::Ready(())
    }

    /// The host closed the stream.
    fn close_clock_time_map(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

    fn sideband_clock_time_map(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _sideband: ww_client_server::StreamSideband,
    ) -> Option<ww_client_server::StreamSideband> {
        // TODO: None sends no reply
        None
    }

    fn clock_configure_sync(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _config: ww_timebase::SyncConfig,
    ) -> RpcResult<Result<(), ww_timebase::ClockError<'_>>> {
        Unimplemented.into()
    }

    fn clock_sync_config(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> RpcResult<ww_timebase::SyncConfig> {
        Unimplemented.into()
    }

    fn clock_sync_status(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> RpcResult<ww_timebase::SyncStatus<'_>> {
        Unimplemented.into()
    }

    fn clock_set_time_of_day(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _t: ww_timebase::DateTime,
    ) -> RpcResult<Result<(), ww_timebase::ClockError<'_>>> {
        Unimplemented.into()
    }
}

pub mod api_impl {
    wire_weaver::ww_codegen!(
        global_paths_api :: PathBenchDevice for super::ServerState,
        server = true, no_alloc = true, use_async = false,
        method_model = "_=immediate",
        property_model = "_=get_set",
        omit = ["clock.host_sync"],
    );
}

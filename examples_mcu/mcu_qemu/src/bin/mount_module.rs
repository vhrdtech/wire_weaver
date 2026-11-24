//! A mount slot served by a module in the firmware's own process (SRV-81): a `ModuleSlot` in a `static` holds the hub
//! of `mount_backend_api`, a blocking `no_alloc` server linked into the firmware, at the slot `[2]` of a parent.
//!
//! Each step is bracketed by `mark_start` / `mark_end` for `qemu_icount`: the first bracket is the empty calibration
//! pair, then `load` (decodes the model and hashes it), `presence`, `forward_blocking` of an `add` twice (cold, warm),
//! `forward_blocking` of the `ticks` stream open, `poll_blocking` with one tick queued, then global trait paths (SRV-82):
//! `ModuleTraits::request_into_mount` of an absolute request (what every request of a server with slots pays) and of
//! a `GlobalFull` path to the hub's `Lamp` (the parent's own table first, then the module's model walked), and
//! `forward_blocking` of the rewritten request. Each step also prints its
//! stack high-water mark below `main`'s frame, measured by painting the stack below it, and the bytes it sent to the
//! parent's event out. The size of the slot comes first.
#![no_main]
#![no_std]

extern crate panic_semihosting;

use core::fmt::Write;
use core::hint::black_box;
use cortex_m_rt::{ExceptionFrame, entry, exception};
use cortex_m_semihosting::{debug, hio};
use static_cell::StaticCell;
use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::UVlq32Backfill;
use wire_weaver::shrink_wrap::tail_bytes::TailBytes;
use ww_client_server::mount::{MountAnswer, MountRequest};
use ww_client_server::dispatch::{GlobalTrait, global_full_path};
use ww_client_server::{PathKind, Priority, Request, RequestKind, ResourceIndex, resource_path};
use ww_device::mount::{Module, ModuleError, ModuleSlot, ModuleTraits, Outbox};

/// The longest message in and out of the slot.
const MSG: usize = 128;
/// Bytes of the module's events per turn.
const OUT: usize = 256;
/// Bytes of stack painted below `main`'s frame before each measured step: more than any step here takes.
const PAINT_BYTES: usize = 16 * 1024;
/// The paint pattern.
const PAINT: u32 = 0xA5A5_A5A5;

/// The hub as a module: `add` answers at once, the `ticks` stream sends one tick per poll while it is open.
pub struct HubModule {
    lamp_on: bool,
    ticking: bool,
    observe: hub_impl::ObserveState,
}

impl HubModule {
    const fn new() -> Self {
        HubModule {
            lamp_on: false,
            ticking: false,
            observe: hub_impl::ObserveState::new(ww_stream::observe::ObserveLimits::DEFAULT),
        }
    }

    fn add(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        a: u32,
        b: u32,
    ) -> RpcResult<u32> {
        Ready(a.wrapping_add(b))
    }

    fn hang(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
        Deferred
    }

    fn ticks(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
        self.ticking = true;
        Ready(())
    }

    fn close_ticks(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {
        self.ticking = false;
    }

    fn sideband_ticks(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        sideband: ww_client_server::StreamSideband,
    ) -> Option<ww_client_server::StreamSideband> {
        Some(sideband)
    }

    fn stray(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>, _seq: u32) -> RpcResult<()> {
        Deferred
    }

    fn lamp_toggle(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<bool> {
        self.lamp_on = !self.lamp_on;
        Ready(self.lamp_on)
    }

    fn get_lamp_on(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> GetResult<bool, CommonError<'_>> {
        Value(self.lamp_on)
    }

    fn observe_state(&mut self) -> &mut hub_impl::ObserveState {
        &mut self.observe
    }
}

mod hub_impl {
    wire_weaver::ww_codegen!(
        mount_backend_api :: Hub for super::HubModule,
        server = true, no_alloc = true, use_async = false,
        method_model = "hang=deferred, stray=deferred, _=immediate",
        property_model = "_=get_set",
        introspect = "no_docs",
    );
}

impl Module for HubModule {
    fn model(&self) -> &[u8] {
        &hub_impl::WW_SELF_BYTES_NO_DOCS
    }

    fn process<'r>(
        &mut self,
        request: &[u8],
        events: &mut Outbox<'_>,
        reply: &'r mut [u8],
    ) -> Result<&'r [u8], ModuleError> {
        self.process_request_bytes(request, reply, events, ())
            .map_err(|_| ModuleError::ReplyTooLong)
    }

    fn poll(&mut self, events: &mut Outbox<'_>) -> Result<(), ModuleError> {
        if self.ticking {
            // a tick that doesn't fit the outbox is counted there
            _ = hub_impl::stream_data_ser().ticks_send_blocking(&[1], events);
        }
        Ok(())
    }
}

/// The slot, set up in place by `ModuleSlot::init`: in `.bss`, never on the stack.
static SLOT: StaticCell<ModuleSlot<HubModule, MSG, OUT>> = StaticCell::new();

/// The parent's event out: counts the bytes it is handed.
struct Sink {
    bytes: usize,
    scratch: [u8; MSG],
}

impl BlockingEventOut for Sink {
    fn send_with_blocking<F>(&mut self, ser: F) -> Result<(), wire_weaver::SendError>
    where
        F: FnOnce(&mut [u8]) -> Result<usize, wire_weaver::shrink_wrap::Error>,
    {
        let len = ser(&mut self.scratch).map_err(wire_weaver::SendError::Serialize)?;
        self.bytes += len;
        black_box(&self.scratch);
        Ok(())
    }
}

/// The slot's `api` in the parent.
const API_PATH: [ResourceIndex; 2] = resource_path([2, 1]);

/// The parent's own traits, as its generated `has_global_trait` looks them up: its root only.
const PARENT_TRAITS: &[GlobalTrait] = &[GlobalTrait {
    hash: ww_global::trait_hash("parent_api", "Parent"),
    major: 0,
    minor: 1,
    path: &[],
}];

fn parent_has(path: &PathKind<'_>) -> bool {
    match path {
        PathKind::GlobalFull {
            gid, trait_name, ..
        } => global_full_path(PARENT_TRAITS, gid, trait_name).is_some(),
        _ => false,
    }
}

/// The parent backend's `global_into_mount`: `request` into the hub slot, written into `buf`.
#[inline(never)]
fn into_mount<'b>(
    slot: &ModuleSlot<HubModule, MSG, OUT>,
    request: &[u8],
    buf: &'b mut [u8],
) -> Option<&'b [u8]> {
    ModuleTraits::<1>::new()
        .slot(&API_PATH, slot)
        .request_into_mount(request, parent_has, buf)
}

/// Hand `request` to the slot as the parent's `hub_request` handler would.
#[inline(never)]
fn forward(
    slot: &mut ModuleSlot<HubModule, MSG, OUT>,
    sink: &mut Sink,
    request: &Request<'_>,
    below: &[ResourceIndex],
) -> MountAnswer {
    let mut cx = Context::new(request.seq.0, (), sink);
    slot.forward_blocking(
        &mut cx,
        &MountRequest {
            request,
            path: below,
        },
    )
}

#[exception]
unsafe fn HardFault(_frame: &ExceptionFrame) -> ! {
    debug::exit(debug::EXIT_FAILURE);
    loop {}
}

#[entry]
fn main() -> ! {
    let mut stdout = hio::hstdout().expect("semihosting stdout");
    let slot = ModuleSlot::init(SLOT.uninit());
    let mut sink = Sink {
        bytes: 0,
        scratch: [0; MSG],
    };
    writeln!(
        stdout,
        "ModuleSlot<Hub, {MSG}, {OUT}>: {} B, model {} B",
        core::mem::size_of::<ModuleSlot<HubModule, MSG, OUT>>(),
        hub_impl::WW_SELF_BYTES_NO_DOCS.len()
    )
    .expect("semihosting write");
    let mut args = [0u8; 16];
    let args_len = {
        let a = 40u32.to_ww_bytes(&mut args).expect("fits").len();
        let mut rest = [0u8; 8];
        let b = 2u32.to_ww_bytes(&mut rest).expect("fits");
        args[a..a + b.len()].copy_from_slice(b);
        a + b.len()
    };
    let add_path = resource_path([2, 0]);
    let add_below: [ResourceIndex; 1] = resource_path([0]);
    let add = Request {
        seq: UVlq32Backfill(7),
        priority: Priority::NORMAL.to_wire(),
        context: Default::default(),
        path_kind: PathKind::absolute(&add_path),
        kind: RequestKind::Call {
            args: TailBytes(&args[..args_len]),
        },
    };
    let ticks_path = resource_path([2, 3]);
    let ticks_below: [ResourceIndex; 1] = resource_path([3]);
    let ticks = Request {
        seq: UVlq32Backfill(8),
        priority: Priority::NORMAL.to_wire(),
        context: Default::default(),
        path_kind: PathKind::absolute(&ticks_path),
        kind: RequestKind::StreamOpen {
            resume: None,
            max_age: None,
            raw: false,
            args: TailBytes(&[]),
        },
    };

    mark_start();
    mark_end();

    let top = paint();
    mark_start();
    let loaded = slot.load(black_box(HubModule::new())).is_ok();
    mark_end();
    let stack = high_water(top);
    writeln!(stdout, "load: ok {loaded}, stack {stack} B").expect("semihosting write");

    let top = paint();
    mark_start();
    let up = matches!(
        black_box(slot.presence()),
        wire_weaver::ww_api::mount::MountPresence::Up { .. }
    );
    mark_end();
    let stack = high_water(top);
    writeln!(stdout, "presence: up {up}, stack {stack} B").expect("semihosting write");

    for run in 0..2 {
        sink.bytes = 0;
        let top = paint();
        mark_start();
        let answer = forward(slot, &mut sink, black_box(&add), &add_below);
        mark_end();
        let stack = high_water(top);
        writeln!(
            stdout,
            "forward add run={run}: {answer:?}, sent {} B, stack {stack} B",
            sink.bytes
        )
        .expect("semihosting write");
    }

    sink.bytes = 0;
    let top = paint();
    mark_start();
    let answer = forward(slot, &mut sink, black_box(&ticks), &ticks_below);
    mark_end();
    let stack = high_water(top);
    writeln!(
        stdout,
        "forward ticks open: {answer:?}, sent {} B, stack {stack} B",
        sink.bytes
    )
    .expect("semihosting write");

    sink.bytes = 0;
    let top = paint();
    mark_start();
    slot.poll_blocking(&mut sink);
    mark_end();
    let stack = high_water(top);
    writeln!(
        stdout,
        "poll one tick: sent {} B, stack {stack} B",
        sink.bytes
    )
    .expect("semihosting write");

    let mut add_bytes = [0u8; MSG];
    let add_bytes = add.to_ww_bytes(&mut add_bytes).expect("fits");
    let toggle_below: [ResourceIndex; 1] = resource_path([0]);
    let lamp = Request {
        seq: UVlq32Backfill(9),
        priority: Priority::NORMAL.to_wire(),
        context: Default::default(),
        path_kind: PathKind::GlobalFull {
            gid: mount_backend_api::HUB_FULL_GID,
            trait_name: "Lamp",
            path_from_trait: wire_weaver::shrink_wrap::RefVec::Slice {
                slice: &toggle_below,
            },
        },
        kind: RequestKind::Call {
            args: TailBytes(&[]),
        },
    };
    let mut lamp_bytes = [0u8; MSG];
    let lamp_bytes = lamp.to_ww_bytes(&mut lamp_bytes).expect("fits");
    let mut buf = [0u8; MSG];

    let top = paint();
    mark_start();
    let absolute = into_mount(slot, black_box(add_bytes), &mut buf).is_some();
    mark_end();
    let stack = high_water(top);
    writeln!(stdout, "global hook, absolute: rewritten {absolute}, stack {stack} B")
        .expect("semihosting write");

    let top = paint();
    mark_start();
    let rewritten = into_mount(slot, black_box(lamp_bytes), &mut buf).map(<[u8]>::len);
    mark_end();
    let stack = high_water(top);
    writeln!(stdout, "global hook, Lamp: rewritten {rewritten:?} B, stack {stack} B")
        .expect("semihosting write");

    let rewritten = into_mount(slot, lamp_bytes, &mut buf).expect("rewritten");
    let rewritten = Request::from_ww_bytes(rewritten).expect("a request");
    let PathKind::Absolute { path } = rewritten.path_kind else {
        panic!("an absolute path");
    };
    let mut below = [ResourceIndex::new(wire_weaver::shrink_wrap::UNib32(0)); 4];
    let mut below_len = 0;
    for id in path.iter().skip(API_PATH.len()) {
        below[below_len] = id.expect("decodes");
        below_len += 1;
    }
    sink.bytes = 0;
    let top = paint();
    mark_start();
    let answer = forward(slot, &mut sink, black_box(&rewritten), &below[..below_len]);
    mark_end();
    let stack = high_water(top);
    writeln!(
        stdout,
        "forward Lamp toggle {:?}: {answer:?}, sent {} B, stack {stack} B",
        &below[..below_len],
        sink.bytes
    )
    .expect("semihosting write");
    writeln!(stdout, "counters: {:?}", slot.counters()).expect("semihosting write");

    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}

/// Paints [PAINT_BYTES] below the caller's stack pointer and returns that stack pointer.
#[inline(never)]
fn paint() -> usize {
    let top = cortex_m::register::msp::read() as usize;
    // this function's own frame sits right below `top`: left alone
    for addr in (top - PAINT_BYTES..top - 64).step_by(4) {
        // SAFETY: free stack below the live frames (64 KB of RAM, the bench's frames take a few KB), written
        // before the measured step uses it
        unsafe { (addr as *mut u32).write_volatile(PAINT) }
    }
    top
}

/// Bytes below `top` written since [paint]: the deepest stack of the steps in between.
#[inline(never)]
fn high_water(top: usize) -> usize {
    let mut addr = top - PAINT_BYTES;
    // SAFETY: the same painted range as in `paint`
    while addr < top && unsafe { (addr as *const u32).read_volatile() } == PAINT {
        addr += 4;
    }
    top - addr
}

#[inline(never)]
fn mark_start() {
    // SAFETY: a `nop` touches no memory, stack or flags
    unsafe { core::arch::asm!("nop", options(nostack, preserves_flags)) }
}

#[inline(never)]
fn mark_end() {
    // SAFETY: two `nop`s touch no memory, stack or flags
    unsafe { core::arch::asm!("nop", "nop", options(nostack, preserves_flags)) }
}

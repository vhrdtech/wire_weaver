//! What each interpreter allocates while it serves a call, counted by a global allocator: a module is untrusted, and
//! an allocation that fails inside a call panics the firmware, so a call should allocate nothing (SRV-87).

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use wire_weaver::shrink_wrap::prelude::*;
use wire_weaver::shrink_wrap::tail_bytes::TailBytes;
use wire_weaver::shrink_wrap::{RefVec, UNib32, UVlq32Backfill};
use wire_weaver::{BlockingEventOut, Context, Priority, SendError};
use ww_client_server::mount::{MountAnswer, MountRequest};
use ww_client_server::{PathKind, Request, RequestKind, ResourceIndex, resource_path};
use ww_device::mount::{Module, ModuleSlot};
use ww_wasm::wasefire::WasefireModule;
use ww_wasm::wasmi::{Limits, WasmiModule};

static CALC: &[u8] = include_bytes!("../../examples_mcu/usb_stm32h725ig/modules/calc.wasm");

const API: [ResourceIndex; 2] = resource_path([0, 1]);
const MSG: usize = 512;
const OUT: usize = 1024;
const ADD: u32 = 0;

thread_local! {
    /// Allocations on this thread while counting, `None` when not counting.
    static COUNT: Cell<Option<usize>> = const { Cell::new(None) };
}

struct Counting;

// SAFETY: every call goes to System as it came; the count is bookkeeping only
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        COUNT.with(|c| c.set(c.get().map(|n| n + 1)));
        // SAFETY: the caller's contract, passed on
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller's contract, passed on
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        COUNT.with(|c| c.set(c.get().map(|n| n + 1)));
        // SAFETY: the caller's contract, passed on
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOC: Counting = Counting;

/// Takes what the slot sends without allocating.
struct Discard;

impl BlockingEventOut for Discard {
    fn send_with_blocking<F>(&mut self, ser: F) -> Result<(), SendError>
    where
        F: FnOnce(&mut [u8]) -> Result<usize, ShrinkWrapError>,
    {
        let mut scratch = [0u8; MSG];
        ser(&mut scratch)?;
        Ok(())
    }

    fn send_reply_bytes_blocking(&mut self, _: Priority, _: &[u8]) -> Result<(), SendError> {
        Ok(())
    }
}

#[derive_shrink_wrap(borrowed)]
struct TwoArgs {
    a: u32,
    b: u32,
}

/// Allocations of each of `calls` calls of `add` into a slot holding `module`.
fn allocations_per_call<M: Module>(module: M, calls: u32) -> Vec<usize> {
    let mut slot = Box::new(ModuleSlot::<M, MSG, OUT>::new());
    assert!(slot.load(module).is_ok());
    let mut path = API.to_vec();
    path.push(ResourceIndex::new(UNib32(ADD)));
    let mut counts = vec![];
    for seq in 1..=calls {
        let mut args = [0u8; 32];
        let args = TwoArgs { a: seq, b: 1 }.to_ww_bytes(&mut args).unwrap();
        let request = Request {
            seq: UVlq32Backfill(seq),
            priority: Priority::NORMAL.to_wire(),
            context: Default::default(),
            path_kind: PathKind::Absolute {
                path: RefVec::Slice { slice: &path },
            },
            kind: RequestKind::Call {
                args: TailBytes(args),
            },
        };
        let mount = MountRequest {
            request: &request,
            path: &path[API.len()..],
        };
        let mut out = Discard;
        let mut cx = Context::new(seq, (), &mut out);
        COUNT.with(|c| c.set(Some(0)));
        let answer = slot.forward_blocking(&mut cx, &mount);
        slot.poll_blocking(&mut out);
        counts.push(COUNT.with(|c| c.replace(None)).unwrap_or(0));
        assert_eq!(answer, MountAnswer::Forwarded);
        assert!(slot.is_loaded());
    }
    counts
}

/// wasmi allocates everything a call needs at load. Only its frame stack is left: wasmi grows it with an infallible
/// `Vec::push` the first time a call reaches a new depth, at most `Limits::recursion` frames (SRV-87).
#[test]
fn wasmi_allocates_only_its_frame_stack() {
    let module = WasmiModule::new(CALC, Limits::default()).unwrap();
    let counts = allocations_per_call(module, 10);
    assert!(counts[0] <= 1, "the frame stack grows once: {counts:?}");
    assert_eq!(counts[1..], [0; 9]);
}

/// wasefire-interpreter allocates its thread, frames, locals and values on every call, with infallible `Vec`
/// pushes (SRV-87): the same number each time, never growing.
#[test]
fn wasefire_allocates_per_call() {
    let module = WasefireModule::new(CALC, 8 * 1024).unwrap();
    let counts = allocations_per_call(module, 10);
    assert!(counts[0] > 0);
    assert!(counts.iter().all(|&n| n == counts[0]), "{counts:?}");
}

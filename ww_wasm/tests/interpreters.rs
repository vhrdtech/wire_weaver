//! The example module images (`examples_mcu/wasm_module`, built by `just wasm-modules` into the firmware's
//! `modules/`) in a `ModuleSlot` on each interpreter: calls, a reload, a trap and a loop, bad binaries.

use wire_weaver::shrink_wrap::prelude::*;
use wire_weaver::shrink_wrap::tail_bytes::TailBytes;
use wire_weaver::shrink_wrap::{RefVec, UNib32, UVlq32Backfill};
use wire_weaver::ww_api::mount::MountPresence;
use wire_weaver::{BlockingEventOut, Context, Priority, SendError};
use ww_client_server::mount::{MountAnswer, MountRequest};
use ww_client_server::{
    ErrorKind, Event, EventKind, PathKind, Request, RequestKind, ResourceIndex, resource_path,
};
use ww_device::mount::{Module, ModuleSlot};
use ww_wasm::wasefire::WasefireModule;
use ww_wasm::wasmi::{Limits, WasmiModule};
use ww_wasm::{BinaryError, LoadError, model_section};

static CALC: &[u8] = include_bytes!("../../examples_mcu/usb_stm32h725ig/modules/calc.wasm");
static CALC_EVOLVED: &[u8] =
    include_bytes!("../../examples_mcu/usb_stm32h725ig/modules/calc_evolved.wasm");

/// The slot's `api` on the front.
const API: [ResourceIndex; 2] = resource_path([0, 1]);
const MSG: usize = 512;
const OUT: usize = 1024;
/// Memory a wasefire module gets: its 4 KiB stack and its data.
const WASEFIRE_MEMORY: usize = 8 * 1024;

/// Ids of `Calc`'s methods.
const ADD: u32 = 0;
const TRAP: u32 = 1;
const SPIN: u32 = 2;
const MUL: u32 = 3;

#[derive_shrink_wrap(borrowed)]
struct TwoArgs {
    a: u32,
    b: u32,
}

#[derive(Default)]
struct Collect(Vec<Vec<u8>>);

impl BlockingEventOut for Collect {
    fn send_with_blocking<F>(&mut self, ser: F) -> Result<(), SendError>
    where
        F: FnOnce(&mut [u8]) -> Result<usize, ShrinkWrapError>,
    {
        let mut scratch = [0u8; MSG];
        let len = ser(&mut scratch)?;
        self.0.push(scratch[..len].to_vec());
        Ok(())
    }

    fn send_reply_bytes_blocking(
        &mut self,
        _priority: Priority,
        message: &[u8],
    ) -> Result<(), SendError> {
        self.0.push(message.to_vec());
        Ok(())
    }
}

/// What came back for a request.
#[derive(Debug, PartialEq, Eq)]
enum Answer {
    Value(u32),
    Error(&'static str),
}

fn call<M: Module>(
    slot: &mut ModuleSlot<M, MSG, OUT>,
    seq: u32,
    method: u32,
    a: u32,
    b: u32,
) -> Answer {
    let mut args = [0u8; 32];
    let args = TwoArgs { a, b }.to_ww_bytes(&mut args).unwrap();
    let mut path = API.to_vec();
    path.push(ResourceIndex::new(UNib32(method)));
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
    let mut out = Collect::default();
    let mut cx = Context::new(seq, (), &mut out);
    // an empty slot: the parent's handler answers `Unavailable` itself
    if slot.forward_blocking(&mut cx, &mount) == MountAnswer::Unavailable {
        return Answer::Error("unavailable");
    }
    slot.poll_blocking(&mut out);
    let [answer] = out.0.as_slice() else {
        panic!("one answer, got {:?}", out.0);
    };
    let event = Event::from_ww_bytes(answer).unwrap();
    assert_eq!(event.seq.0, seq);
    match event.result {
        Ok(EventKind::Value { data }) => Answer::Value(u32::from_ww_bytes(data.0).unwrap()),
        Ok(other) => panic!("unexpected {other:?}"),
        Err(e) => Answer::Error(match e.kind {
            ErrorKind::Unavailable => "unavailable",
            ErrorKind::ResponseSerFailed => "too long",
            ErrorKind::BadPath => "bad path",
            _ => "other",
        }),
    }
}

fn crate_name<M: Module>(slot: &ModuleSlot<M, MSG, OUT>) -> Option<String> {
    match slot.presence() {
        MountPresence::Up { api, .. } => Some(api.crate_id.to_string()),
        _ => None,
    }
}

fn wasefire(wasm: &'static [u8]) -> WasefireModule {
    WasefireModule::new(wasm, WASEFIRE_MEMORY).unwrap()
}

fn wasmi(wasm: &'static [u8]) -> WasmiModule {
    WasmiModule::new(wasm, Limits::default()).unwrap()
}

/// Calls, a reload with another API, and a trap on one interpreter.
fn calls_reload_and_trap<M: Module>(load: fn(&'static [u8]) -> M) {
    let mut slot = Box::new(ModuleSlot::<M, MSG, OUT>::new());
    assert!(slot.load(load(CALC)).is_ok());
    assert_eq!(crate_name(&slot).as_deref(), Some("wasm_module_api"));
    assert_eq!(call(&mut slot, 1, ADD, 2, 3), Answer::Value(5));
    assert_eq!(call(&mut slot, 2, ADD, u32::MAX, 2), Answer::Value(1));
    assert_eq!(call(&mut slot, 3, MUL, 2, 3), Answer::Error("bad path"));

    assert!(slot.load(load(CALC_EVOLVED)).is_ok());
    assert!(slot.take_changed());
    assert_eq!(
        crate_name(&slot).as_deref(),
        Some("wasm_module_api_evolved")
    );
    assert_eq!(call(&mut slot, 4, MUL, 6, 7), Answer::Value(42));

    assert_eq!(call(&mut slot, 5, TRAP, 0, 0), Answer::Error("unavailable"));
    assert!(!slot.is_loaded(), "a trap unloads the module");
    assert_eq!(slot.counters().faults, 1);
    assert_eq!(call(&mut slot, 6, ADD, 1, 1), Answer::Error("unavailable"));

    assert!(slot.load(load(CALC)).is_ok());
    assert_eq!(call(&mut slot, 7, ADD, 1, 1), Answer::Value(2));
}

#[test]
fn wasefire_calls_reload_and_trap() {
    calls_reload_and_trap(wasefire);
}

#[test]
fn wasmi_calls_reload_and_trap() {
    calls_reload_and_trap(wasmi);
}

/// wasmi runs out of fuel in an endless loop: the module is unloaded, the slot goes on. (wasefire-interpreter has no
/// fuel: the same call never returns, SRV-86.)
#[test]
fn wasmi_stops_a_loop() {
    let mut slot = Box::new(ModuleSlot::<WasmiModule, MSG, OUT>::new());
    let limits = Limits {
        fuel: 100_000,
        ..Limits::default()
    };
    assert!(slot.load(WasmiModule::new(CALC, limits).unwrap()).is_ok());
    assert_eq!(call(&mut slot, 1, SPIN, 0, 0), Answer::Error("unavailable"));
    assert!(!slot.is_loaded());
    assert_eq!(slot.counters().faults, 1);
}

/// Each image has its model in its custom section, the same bytes the slot reads.
#[test]
fn models_come_from_the_custom_section() {
    let module = wasmi(CALC);
    assert_eq!(module.model(), model_section(CALC).unwrap());
    assert!(!module.model().is_empty());
}

#[test]
fn bad_binaries_are_refused() {
    for (wasm, error) in [
        (&b"not wasm"[..], LoadError::Binary(BinaryError::Header)),
        // a valid empty module: no model
        (
            &b"\0asm\x01\0\0\0"[..],
            LoadError::Binary(BinaryError::Model),
        ),
    ] {
        assert_eq!(
            WasefireModule::new(wasm, WASEFIRE_MEMORY).err(),
            Some(error)
        );
        assert_eq!(WasmiModule::new(wasm, Limits::default()).err(), Some(error));
    }
    // the model, but nothing else of the ABI
    let mut model_only = b"\0asm\x01\0\0\0".to_vec();
    model_only.extend([0, 9, 8]);
    model_only.extend(b"ww_model");
    let model_only: &'static [u8] = Box::leak(model_only.into_boxed_slice());
    let memory = LoadError::Binary(BinaryError::Memory);
    assert_eq!(
        WasefireModule::new(model_only, WASEFIRE_MEMORY).err(),
        Some(memory)
    );
    assert_eq!(
        WasmiModule::new(model_only, Limits::default()).err(),
        Some(memory)
    );
}

/// A memory too small for the module's data is refused at load, not met by a fault later.
#[test]
fn wasefire_refuses_a_memory_too_small() {
    assert_eq!(
        WasefireModule::new(CALC, 1024).err(),
        Some(LoadError::Instantiate)
    );
}

/// A module that grows its memory past `Limits::memory` on a call: wasmi refuses the growth and traps, and the slot
/// unloads the module; nothing is allocated for it.
#[test]
fn wasmi_traps_on_memory_growth_past_the_limit() {
    let model: String = model_section(CALC)
        .unwrap()
        .iter()
        .map(|b| format!("\\{b:02x}"))
        .collect();
    let wat = format!(
        r#"(module
            (memory (export "memory") 1)
            (func (export "ww_request") (result i32) i32.const 1024)
            (func (export "ww_reply") (result i32) i32.const 2048)
            (func (export "ww_buf_len") (result i32) i32.const 512)
            (func (export "ww_process") (param i32) (result i32)
                (if (i32.eq (memory.grow (i32.const 1)) (i32.const -1)) (then (return (i32.const 0))))
                i32.const 0)
            (@custom "ww_model" "{model}"))"#
    );
    let wasm: &'static [u8] = Box::leak(wat::parse_str(wat).unwrap().into_boxed_slice());
    let limits = Limits {
        memory: 64 * 1024,
        ..Limits::default()
    };
    let mut slot = Box::new(ModuleSlot::<WasmiModule, MSG, OUT>::new());
    assert!(slot.load(WasmiModule::new(wasm, limits).unwrap()).is_ok());
    assert_eq!(call(&mut slot, 1, ADD, 1, 1), Answer::Error("unavailable"));
    assert!(!slot.is_loaded(), "the growth trapped");
    assert_eq!(slot.counters().faults, 1);
}

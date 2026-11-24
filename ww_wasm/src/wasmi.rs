//! A module run by wasmi as a `ww_device::mount::Module`. wasmi meters fuel: a module that loops is stopped and
//! unloaded, never hangs its parent. Everything a call needs is allocated at load: every function is translated, the
//! stack and the event queue get their whole size, and memory or table growth past [Limits] traps. A call allocates
//! only when the module grows its memory or a table, and wasmi reserves that fallibly: a failed growth traps.

use alloc::vec::Vec;

use ::wasmi::{
    CompilationMode, Config, Engine, Linker, Memory, Module as WasmiBinary, Store, StoreLimits,
    StoreLimitsBuilder, TypedFunc,
};
use ww_device::mount::{Module, ModuleError, Outbox};

use crate::{
    Buffers, EventStatus, IMPORT_EVENT, IMPORT_MODULE, LoadError, Status, export, guest_range,
    has_memory_export, model_section,
};

/// What a [WasmiModule] may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Fuel per call into the module (`ww_process`, `ww_poll`): about one per wasm instruction. A call that runs out
    /// traps, and the slot unloads the module.
    pub fuel: u64,
    /// Bytes of the events one call may queue; more are dropped. The slot's `OUT` is a good value.
    pub events: usize,
    /// Bytes of wasmi's value stack (the module's locals and operands, kept on the heap): deeper calls trap.
    pub stack: usize,
    /// Calls deep: deeper calls trap.
    pub recursion: usize,
    /// Bytes the module's memory may grow to: `memory.grow` past it fails and traps. Size it to what the heap has
    /// room for; a module whose memory starts larger isn't loaded.
    pub memory: usize,
    /// Elements a table may grow to, as `memory` for memory.
    pub table: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            fuel: 1_000_000,
            events: 1024,
            stack: 16 * 1024,
            recursion: 64,
            memory: 64 * 1024,
            table: 1024,
        }
    }
}

/// The store's data: the events a call queues, each behind its length (`u16`, little endian), until the host hands
/// them to the slot, and the limits of the module's memory and tables.
struct Queued {
    /// Allocated to `cap` at load, never grown.
    bytes: Vec<u8>,
    cap: usize,
    limits: StoreLimits,
}

/// A wasm module run by wasmi: the module ABI (crate docs) as a `ww_device::mount::Module`.
pub struct WasmiModule {
    store: Store<Queued>,
    memory: Memory,
    process: TypedFunc<i32, i32>,
    poll: Option<TypedFunc<(), ()>>,
    buffers: Buffers,
    model: &'static [u8],
    fuel: u64,
}

impl WasmiModule {
    /// Loads `wasm`, a module binary in flash: validates and instantiates it with its own engine.
    ///
    /// # Errors
    ///
    /// [LoadError] when the binary doesn't parse, has no model, isn't valid, doesn't link with the host or doesn't
    /// export the ABI.
    pub fn new(wasm: &'static [u8], limits: Limits) -> Result<Self, LoadError> {
        let model = model_section(wasm)?;
        has_memory_export(wasm)?;
        let mut config = Config::default();
        config
            .consume_fuel(true)
            // wasmi's defaults (1 MB of stack, 1000 calls deep) would let a module take all of a firmware's heap
            // the whole stack at once: a call never grows it
            .set_min_stack_height(limits.stack)
            .set_max_stack_height(limits.stack)
            .set_max_recursion_depth(limits.recursion)
            .set_max_cached_stacks(1)
            // lazy translation would allocate inside a module's first call of each function
            .compilation_mode(CompilationMode::Eager);
        let engine = Engine::new(&config);
        let binary = WasmiBinary::new(&engine, wasm).map_err(|_| LoadError::Invalid)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(limits.events)
            .map_err(|_| LoadError::Instantiate)?;
        let queued = Queued {
            bytes,
            cap: limits.events,
            limits: StoreLimitsBuilder::new()
                .memory_size(limits.memory)
                .table_elements(limits.table)
                .instances(1)
                .memories(1)
                .tables(1)
                .trap_on_grow_failure(true)
                .build(),
        };
        let mut store = Store::new(&engine, queued);
        store.limiter(|queued| &mut queued.limits);
        store
            .set_fuel(limits.fuel)
            .map_err(|_| LoadError::Instantiate)?;
        let mut linker = Linker::<Queued>::new(&engine);
        linker
            .func_wrap(
                IMPORT_MODULE,
                IMPORT_EVENT,
                |caller: ::wasmi::Caller<'_, Queued>, ptr: i32, len: i32| -> i32 {
                    event(caller, ptr, len)
                },
            )
            .map_err(|_| LoadError::Instantiate)?;
        let instance = linker
            .instantiate_and_start(&mut store, &binary)
            .map_err(|_| LoadError::Instantiate)?;
        let memory = instance
            .get_memory(&store, export::MEMORY)
            .ok_or(LoadError::Export)?;
        let get = |store: &Store<Queued>, name| {
            instance
                .get_typed_func::<(), i32>(store, name)
                .map_err(|_| LoadError::Export)
        };
        let (request, reply, len) = (
            get(&store, export::REQUEST)?,
            get(&store, export::REPLY)?,
            get(&store, export::BUF_LEN)?,
        );
        let mut call = |f: TypedFunc<(), i32>| f.call(&mut store, ()).map_err(|_| LoadError::Trap);
        let (request, reply, len) = (call(request)?, call(reply)?, call(len)?);
        let buffers = Buffers::new(request, reply, len, memory.data_size(&store))?;
        let process = instance
            .get_typed_func::<i32, i32>(&store, export::PROCESS)
            .map_err(|_| LoadError::Export)?;
        let poll = instance.get_typed_func::<(), ()>(&store, export::POLL).ok();
        Ok(WasmiModule {
            store,
            memory,
            process,
            poll,
            buffers,
            model,
            fuel: limits.fuel,
        })
    }

    /// Fuel left after the last call.
    pub fn fuel_left(&self) -> u64 {
        self.store.get_fuel().unwrap_or(0)
    }

    /// Hands the events the last call queued to `events`.
    fn drain(&mut self, events: &mut Outbox<'_>) {
        let queued = core::mem::take(&mut self.store.data_mut().bytes);
        let mut rest = queued.as_slice();
        while let Some((len, after)) = rest.split_first_chunk::<2>() {
            let len = usize::from(u16::from_le_bytes(*len));
            let Some((event, after)) = after.split_at_checked(len) else {
                break;
            };
            // a full outbox counts the event as dropped
            _ = events.push(event);
            rest = after;
        }
        // keep the allocation for the next call
        let mut queued = queued;
        queued.clear();
        self.store.data_mut().bytes = queued;
    }
}

/// `ww.event`: queues `len` bytes at `ptr` of the module's memory.
fn event(mut caller: ::wasmi::Caller<'_, Queued>, ptr: i32, len: i32) -> i32 {
    let Some(::wasmi::Extern::Memory(memory)) = caller.get_export(export::MEMORY) else {
        return EventStatus::NoRoom as i32;
    };
    let (data, queued) = memory.data_and_store_mut(&mut caller);
    let Some(range) = guest_range(ptr, len, data.len()) else {
        return EventStatus::NoRoom as i32;
    };
    let Ok(len) = u16::try_from(range.len()) else {
        return EventStatus::NoRoom as i32;
    };
    if queued.bytes.len() + 2 + range.len() > queued.cap {
        return EventStatus::NoRoom as i32;
    }
    queued.bytes.extend_from_slice(&len.to_le_bytes());
    queued.bytes.extend_from_slice(&data[range]);
    EventStatus::Queued as i32
}

impl Module for WasmiModule {
    fn model(&self) -> &[u8] {
        self.model
    }

    fn process<'r>(
        &mut self,
        request: &[u8],
        events: &mut Outbox<'_>,
        reply: &'r mut [u8],
    ) -> Result<&'r [u8], ModuleError> {
        let Some(range) = self.buffers.request(request.len()) else {
            return Err(ModuleError::ReplyTooLong);
        };
        let memory = self.memory.data_mut(&mut self.store);
        memory
            .get_mut(range)
            .ok_or(ModuleError::Fault)?
            .copy_from_slice(request);
        self.store
            .set_fuel(self.fuel)
            .map_err(|_| ModuleError::Fault)?;
        // a request is at most a buffer: its length fits an i32
        let len = i32::try_from(request.len()).map_err(|_| ModuleError::ReplyTooLong)?;
        let result = self.process.call(&mut self.store, len);
        self.drain(events);
        let written = result.map_err(|_| ModuleError::Fault)?;
        if written == Status::ReplyTooLong as i32 || written == Status::RequestTooLong as i32 {
            return Err(ModuleError::ReplyTooLong);
        }
        let range = self.buffers.reply(written).ok_or(ModuleError::Fault)?;
        let to = reply
            .get_mut(..range.len())
            .ok_or(ModuleError::ReplyTooLong)?;
        let memory = self.memory.data(&self.store);
        to.copy_from_slice(memory.get(range).ok_or(ModuleError::Fault)?);
        Ok(to)
    }

    fn poll(&mut self, events: &mut Outbox<'_>) -> Result<(), ModuleError> {
        let Some(poll) = self.poll else {
            return Ok(());
        };
        self.store
            .set_fuel(self.fuel)
            .map_err(|_| ModuleError::Fault)?;
        let result = poll.call(&mut self.store, ());
        self.drain(events);
        result.map_err(|_| ModuleError::Fault)
    }
}

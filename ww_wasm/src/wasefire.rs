//! A module run by wasefire-interpreter as a `ww_device::mount::Module`. wasefire-interpreter interprets the wasm
//! binary in place (from flash) and takes a memory of any length, but it meters nothing: a module that loops never
//! returns, and its parent's task hangs with it (SRV-86).

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use core::mem::ManuallyDrop;
use core::ptr::NonNull;

use wasefire_interpreter::{InstId, MEMORY_ALIGN, Module as WasefireBinary, RunResult, Store, Val};
use ww_device::mount::{Module, ModuleError, Outbox};

use crate::{
    Buffers, EventStatus, IMPORT_EVENT, IMPORT_MODULE, LoadError, Status, export, guest_range,
    has_memory_export, model_section,
};

/// 16 bytes of a module's memory: wasefire-interpreter wants the memory aligned to [MEMORY_ALIGN].
#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct Chunk([u8; MEMORY_ALIGN]);

/// A wasm module run by wasefire-interpreter: the module ABI (crate docs) as a `ww_device::mount::Module`.
///
/// The binary must be prepared on the host first (`wasefire_interpreter::prepare`, the `ww-wasm-prepare` tool of
/// this crate): it carries the side table that lets the interpreter run it in place. wasmi ignores that section.
pub struct WasefireModule {
    /// Dropped before `memory` is freed: it borrows it.
    store: ManuallyDrop<Store<'static>>,
    /// The module's memory, on the heap, lent to `store` for as long as it lives.
    memory: NonNull<[Chunk]>,
    inst: Option<InstId>,
    buffers: Buffers,
    model: &'static [u8],
}

impl Drop for WasefireModule {
    fn drop(&mut self) {
        // SAFETY: `store` is dropped once, here, and never used after
        unsafe { ManuallyDrop::drop(&mut self.store) };
        // SAFETY: `memory` came from `Box::leak` in `new`; the store, the only borrower, is gone
        drop(unsafe { Box::from_raw(self.memory.as_ptr()) });
    }
}

impl WasefireModule {
    /// Loads `wasm`, a module binary in flash, with `memory_len` bytes of memory: the module's data and its stack
    /// must fit (a wasm page is 64 KiB, wasefire-interpreter needs only what the module uses; past it, an access
    /// traps).
    ///
    /// # Errors
    ///
    /// [LoadError] when the binary doesn't parse, has no model, isn't valid, doesn't link with the host or doesn't
    /// export the ABI.
    pub fn new(wasm: &'static [u8], memory_len: usize) -> Result<Self, LoadError> {
        let model = model_section(wasm)?;
        // wasefire-interpreter takes memory 0 as there once instantiated: a module without one is refused here
        has_memory_export(wasm)?;
        let binary = WasefireBinary::new(wasm).map_err(|_| LoadError::Invalid)?;
        let chunks: &'static mut [Chunk] = Box::leak(
            vec![Chunk([0; MEMORY_ALIGN]); memory_len.div_ceil(MEMORY_ALIGN)].into_boxed_slice(),
        );
        let memory_ptr = NonNull::from(&mut *chunks);
        // SAFETY: the chunks are plain bytes, `memory_len` of them within the allocation; `chunks` isn't used again,
        // and `memory_ptr` only frees the allocation once the store, the slice's one user, is dropped
        let memory: &'static mut [u8] = unsafe {
            core::slice::from_raw_parts_mut(chunks.as_mut_ptr().cast::<u8>(), memory_len)
        };
        let mut module = WasefireModule {
            store: ManuallyDrop::new(Store::default()),
            memory: memory_ptr,
            inst: None,
            buffers: Buffers {
                request: 0,
                reply: 0,
                len: 0,
            },
            model,
        };
        // on an error from here on, dropping `module` drops the store, then frees the memory
        module
            .store
            .link_func(IMPORT_MODULE, IMPORT_EVENT, 2, 1)
            .map_err(|_| LoadError::Instantiate)?;
        let inst = module
            .store
            .instantiate(binary, memory)
            .map_err(|_| LoadError::Instantiate)?;
        module.inst = Some(inst);
        let mut get = |name| match module.run(inst, name, Vec::new(), None) {
            Ok(Some(value)) => Ok(value),
            Ok(None) => Err(LoadError::Export),
            Err(_) => Err(LoadError::Trap),
        };
        let (request, reply, len) = (
            get(export::REQUEST)?,
            get(export::REPLY)?,
            get(export::BUF_LEN)?,
        );
        module.buffers = Buffers::new(request, reply, len, memory_len)?;
        Ok(module)
    }

    /// Runs the export `name`, handing the events it sends to `events` (dropped without one); its `i32` result.
    /// `Ok(None)`: no such export, or not of that type.
    fn run(
        &mut self,
        inst: InstId,
        name: &str,
        args: Vec<Val>,
        mut events: Option<&mut Outbox<'_>>,
    ) -> Result<Option<i32>, ModuleError> {
        let mut result = match self.store.invoke(inst, name, args) {
            Ok(result) => result,
            Err(wasefire_interpreter::Error::Trap) => return Err(ModuleError::Fault),
            Err(_) => return Ok(None),
        };
        loop {
            match result {
                RunResult::Done(values) => {
                    return Ok(match values.as_slice() {
                        [Val::I32(value)] => Some(*value as i32),
                        [] => Some(0),
                        _ => None,
                    });
                }
                RunResult::Host(mut call) => {
                    // the one host function linked: `ww.event(ptr, len)`
                    let status = match call.args() {
                        [Val::I32(ptr), Val::I32(len)] => {
                            let (ptr, len) = (*ptr as i32, *len as i32);
                            let memory = call.mem_mut();
                            match (guest_range(ptr, len, memory.len()), events.as_deref_mut()) {
                                (Some(range), Some(events)) => match events.push(&memory[range]) {
                                    Ok(()) => EventStatus::Queued,
                                    Err(_) => EventStatus::NoRoom,
                                },
                                _ => EventStatus::NoRoom,
                            }
                        }
                        _ => EventStatus::NoRoom,
                    };
                    result = call
                        .resume(&[Val::I32(status as i32 as u32)])
                        .map_err(|_| ModuleError::Fault)?;
                }
            }
        }
    }
}

impl Module for WasefireModule {
    fn model(&self) -> &[u8] {
        self.model
    }

    fn process<'r>(
        &mut self,
        request: &[u8],
        events: &mut Outbox<'_>,
        reply: &'r mut [u8],
    ) -> Result<&'r [u8], ModuleError> {
        let inst = self.inst.ok_or(ModuleError::Fault)?;
        let Some(range) = self.buffers.request(request.len()) else {
            return Err(ModuleError::ReplyTooLong);
        };
        let memory = self.store.memory(inst).map_err(|_| ModuleError::Fault)?;
        memory
            .get_mut(range)
            .ok_or(ModuleError::Fault)?
            .copy_from_slice(request);
        // a request is at most a buffer: its length fits an i32
        let len = u32::try_from(request.len()).map_err(|_| ModuleError::ReplyTooLong)?;
        let written = self
            .run(inst, export::PROCESS, vec![Val::I32(len)], Some(events))?
            .ok_or(ModuleError::Fault)?;
        if written == Status::ReplyTooLong as i32 || written == Status::RequestTooLong as i32 {
            return Err(ModuleError::ReplyTooLong);
        }
        let range = self.buffers.reply(written).ok_or(ModuleError::Fault)?;
        let to = reply
            .get_mut(..range.len())
            .ok_or(ModuleError::ReplyTooLong)?;
        let memory = self.store.memory(inst).map_err(|_| ModuleError::Fault)?;
        to.copy_from_slice(memory.get(range).ok_or(ModuleError::Fault)?);
        Ok(to)
    }

    fn poll(&mut self, events: &mut Outbox<'_>) -> Result<(), ModuleError> {
        let inst = self.inst.ok_or(ModuleError::Fault)?;
        // a module without `ww_poll` has nothing to do outside of a request
        self.run(inst, export::POLL, Vec::new(), Some(events))
            .map(|_| ())
    }
}

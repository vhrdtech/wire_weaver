//! WireWeaver servers compiled to wasm and served from a mount slot by an interpreter (SRV-85): the module ABI, the
//! guest's side of it ([guest_module!], feature `guest`) and the host's `ww_device::mount::Module` over
//! wasefire-interpreter ([wasefire::WasefireModule], feature `wasefire`) or wasmi ([wasmi::WasmiModule], feature
//! `wasmi`). Everything is `no_std`; the interpreters need `alloc`.
//!
//! # The module ABI
//!
//! A module is a WireWeaver server generated with `use_async = false` and compiled to `wasm32-unknown-unknown`.
//! Everything between it and the host is bytes in the module's memory:
//!
//! | Item                         | Kind          | What it is                                                         |
//! |------------------------------|---------------|--------------------------------------------------------------------|
//! | `memory`                     | export        | the module's linear memory                                         |
//! | `ww_request() -> i32`        | export        | address of the request buffer, [ww_buf_len] bytes                  |
//! | `ww_reply() -> i32`          | export        | address of the reply buffer, [ww_buf_len] bytes                    |
//! | `ww_buf_len() -> i32`        | export        | length of each buffer: the longest request and reply               |
//! | `ww_process(len: i32) -> i32` | export       | serves the request in `request[..len]`: the reply's length in `reply`, or a negative [Status] |
//! | `ww_poll()`                  | export, optional | the module's turn outside of a request                          |
//! | `ww.event(ptr, len) -> i32`  | import        | an event of `len` bytes at `ptr`: 0 queued, 1 no room              |
//! | `ww_model`                   | custom section | the model: the server's `WW_SELF_BYTES_NO_DOCS`                   |
//!
//! The model is a custom section, so the host reads it from the wasm binary in flash with no copy and no call into
//! the module ([model_section]). The host reads the buffers' addresses once, at load, and checks every access
//! against the module's memory: a module can't make the host read or write outside of it.
//!
//! [ww_buf_len]: export::BUF_LEN
#![cfg_attr(not(test), no_std)]

#[cfg(any(feature = "wasefire", feature = "wasmi"))]
extern crate alloc;

mod binary;
#[cfg(feature = "guest")]
pub mod guest;
#[cfg(feature = "wasefire")]
pub mod wasefire;
#[cfg(feature = "wasmi")]
pub mod wasmi;

pub use binary::{BinaryError, has_memory_export, model_section};

/// The custom section that holds a module's model.
pub const MODEL_SECTION: &str = "ww_model";

/// The import module of the host's functions.
pub const IMPORT_MODULE: &str = "ww";
/// The host function a module sends an event with: `(ptr: i32, len: i32) -> i32`.
pub const IMPORT_EVENT: &str = "event";

/// Names of the module's exports.
pub mod export {
    pub const MEMORY: &str = "memory";
    pub const REQUEST: &str = "ww_request";
    pub const REPLY: &str = "ww_reply";
    pub const BUF_LEN: &str = "ww_buf_len";
    pub const PROCESS: &str = "ww_process";
    pub const POLL: &str = "ww_poll";
}

/// What `ww_process` returns instead of a reply's length.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Status {
    /// The reply didn't fit the reply buffer.
    ReplyTooLong = -1,
    /// The request is longer than the request buffer.
    RequestTooLong = -2,
}

/// What `ww.event` returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum EventStatus {
    Queued = 0,
    /// No room for the event in this turn: it is dropped and counted by the slot.
    NoRoom = 1,
}

/// Why an interpreter didn't load a module.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum LoadError {
    /// The wasm binary doesn't parse, has no model, or lacks `memory`.
    Binary(BinaryError),
    /// The interpreter refused the binary: invalid, or a feature it doesn't run (floats on wasefire).
    Invalid,
    /// An import is not the host's, or a memory, table or start function failed.
    Instantiate,
    /// An export of the ABI is missing or has another signature.
    Export,
    /// The buffers `ww_request` and `ww_reply` name are not in the module's memory, or overlap.
    Buffers,
    /// The module trapped while the host asked for its buffers.
    Trap,
}

impl From<BinaryError> for LoadError {
    fn from(error: BinaryError) -> Self {
        LoadError::Binary(error)
    }
}

/// The request and reply buffers of a module, checked against its memory at load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(not(any(feature = "wasefire", feature = "wasmi")), allow(dead_code))]
pub(crate) struct Buffers {
    pub request: usize,
    pub reply: usize,
    pub len: usize,
}

#[cfg(any(feature = "wasefire", feature = "wasmi"))]
impl Buffers {
    /// The buffers at `request` and `reply`, `len` bytes each, when both lie in `memory_len` bytes and don't overlap.
    pub fn new(request: i32, reply: i32, len: i32, memory_len: usize) -> Result<Self, LoadError> {
        let as_usize = |v: i32| usize::try_from(v).map_err(|_| LoadError::Buffers);
        let (request, reply, len) = (as_usize(request)?, as_usize(reply)?, as_usize(len)?);
        let end = |start: usize| start.checked_add(len).filter(|end| *end <= memory_len);
        let (Some(request_end), Some(reply_end)) = (end(request), end(reply)) else {
            return Err(LoadError::Buffers);
        };
        if request < reply_end && reply < request_end {
            return Err(LoadError::Buffers);
        }
        Ok(Buffers {
            request,
            reply,
            len,
        })
    }

    /// The request buffer's range, `len` bytes of it, when they fit.
    pub fn request(&self, len: usize) -> Option<core::ops::Range<usize>> {
        (len <= self.len).then(|| self.request..self.request + len)
    }

    /// The reply buffer's range, `len` bytes of it (`ww_process`'s result), when they fit.
    pub fn reply(&self, len: i32) -> Option<core::ops::Range<usize>> {
        let len = usize::try_from(len).ok().filter(|len| *len <= self.len)?;
        Some(self.reply..self.reply + len)
    }
}

/// A range of `len` bytes at `ptr` in a memory of `memory_len` bytes: an event's bytes the module names.
#[cfg(any(feature = "wasefire", feature = "wasmi"))]
pub(crate) fn guest_range(
    ptr: i32,
    len: i32,
    memory_len: usize,
) -> Option<core::ops::Range<usize>> {
    let ptr = usize::try_from(ptr).ok()?;
    let len = usize::try_from(len).ok()?;
    let end = ptr.checked_add(len).filter(|end| *end <= memory_len)?;
    Some(ptr..end)
}

//! The guest's side of the module ABI: a server generated with `use_async = false` in a [Guest], exported by
//! [guest_module!](crate::guest_module).
//!
//! ```ignore
//! mod server_impl {
//!     wire_weaver::ww_codegen!(
//!         calc_api :: Calc for super::Calc,
//!         server = true, no_alloc = true, use_async = false,
//!         method_model = "_=immediate", introspect = "no_docs"
//!     );
//! }
//! ww_wasm::guest_module!(Calc = Calc, model = server_impl::WW_SELF_BYTES_NO_DOCS, buf_len = 512);
//! ```

use core::cell::UnsafeCell;

use wire_weaver::shrink_wrap::prelude::ShrinkWrapError;
use wire_weaver::{BlockingEventOut, SendError, WireWeaverApiBackend};

use crate::{EventStatus, Status};

#[cfg(target_family = "wasm")]
#[link(wasm_import_module = "ww")]
unsafe extern "C" {
    /// `ww.event`, see the crate docs.
    #[link_name = "event"]
    fn ww_event(ptr: *const u8, len: usize) -> i32;
}

/// Hands an event to the host (`ww.event`). Off wasm there is no host: the event is dropped.
fn send_event(event: &[u8]) -> i32 {
    #[cfg(target_family = "wasm")]
    {
        // SAFETY: the host reads `len` bytes at `ptr` within this module's memory, while the slice is borrowed
        unsafe { ww_event(event.as_ptr(), event.len()) }
    }
    #[cfg(not(target_family = "wasm"))]
    {
        _ = event;
        EventStatus::NoRoom as i32
    }
}

/// The events of one call, serialized into a buffer of the guest's and handed to the host one by one.
struct HostEvents<'b> {
    buf: &'b mut [u8],
}

impl BlockingEventOut for HostEvents<'_> {
    fn send_with_blocking<F>(&mut self, ser: F) -> Result<(), SendError>
    where
        F: FnOnce(&mut [u8]) -> Result<usize, ShrinkWrapError>,
    {
        let len = ser(self.buf).map_err(SendError::Serialize)?;
        let event = self.buf.get(..len).ok_or(SendError::Serialize(
            ShrinkWrapError::OutOfBoundsWriteRawSlice,
        ))?;
        if send_event(event) == EventStatus::Queued as i32 {
            Ok(())
        } else {
            Err(SendError::Serialize(
                ShrinkWrapError::OutOfBoundsWriteRawSlice,
            ))
        }
    }
}

struct Inner<S, const LEN: usize> {
    server: S,
    request: [u8; LEN],
    reply: [u8; LEN],
    events: [u8; LEN],
}

/// A server and its buffers in the guest's memory: what [guest_module!](crate::guest_module) exports. `LEN` is the
/// longest request, reply and event.
pub struct Guest<S, const LEN: usize> {
    inner: UnsafeCell<Inner<S, LEN>>,
}

// SAFETY: wasm32-unknown-unknown has one thread, and the host never calls into the module while an export runs
// (`ww.event` returns without calling back), so the exports never alias `inner`.
unsafe impl<S, const LEN: usize> Sync for Guest<S, LEN> {}

impl<S: WireWeaverApiBackend<Medium = ()>, const LEN: usize> Guest<S, LEN> {
    pub const fn new(server: S) -> Self {
        Guest {
            inner: UnsafeCell::new(Inner {
                server,
                request: [0; LEN],
                reply: [0; LEN],
                events: [0; LEN],
            }),
        }
    }

    /// `ww_request`: the request buffer's address.
    pub fn request_ptr(&self) -> *mut u8 {
        // SAFETY: a raw pointer to a field, no reference made
        unsafe { (&raw mut (*self.inner.get()).request).cast() }
    }

    /// `ww_reply`: the reply buffer's address.
    pub fn reply_ptr(&self) -> *mut u8 {
        // SAFETY: a raw pointer to a field, no reference made
        unsafe { (&raw mut (*self.inner.get()).reply).cast() }
    }

    /// `ww_buf_len`.
    pub const fn buf_len(&self) -> i32 {
        const { assert!(LEN <= i32::MAX as usize) };
        LEN as i32
    }

    /// `ww_process`: serves the request in the request buffer's first `len` bytes; the reply's length in the reply
    /// buffer, or a negative [Status].
    pub fn process(&self, len: usize) -> i32 {
        // SAFETY: see `impl Sync`: no other reference to `inner` exists while an export runs
        let Inner {
            server,
            request,
            reply,
            events,
        } = unsafe { &mut *self.inner.get() };
        let Some(request) = request.get(..len) else {
            return Status::RequestTooLong as i32;
        };
        let mut out = HostEvents { buf: events };
        let Ok(written) = server.process_bytes(&mut out, (), request, reply) else {
            return Status::ReplyTooLong as i32;
        };
        let (written_at, written_len) = (written.as_ptr() as usize, written.len());
        if written_len == 0 {
            return 0;
        }
        // the reply starts the buffer for the host: move it there when the server wrote it further in
        let Some(start) = written_at
            .checked_sub(reply.as_ptr() as usize)
            .filter(|start| start.checked_add(written_len).is_some_and(|end| end <= LEN))
        else {
            return Status::ReplyTooLong as i32;
        };
        reply.copy_within(start..start + written_len, 0);
        written_len as i32
    }
}

/// Exports a server as a module (the crate docs' ABI): `$server` built by `$init` in a `static`, the model
/// `$model` (the generated `WW_SELF_BYTES_NO_DOCS`) in the `ww_model` custom section, buffers of `$len` bytes.
#[macro_export]
macro_rules! guest_module {
    ($server:ty = $init:expr, model = $model:expr, buf_len = $len:expr) => {
        #[unsafe(link_section = "ww_model")]
        #[used]
        static WW_MODEL: [u8; $model.len()] = $model;

        static WW_GUEST: $crate::guest::Guest<$server, { $len }> = $crate::guest::Guest::new($init);

        #[unsafe(no_mangle)]
        pub extern "C" fn ww_request() -> *mut u8 {
            WW_GUEST.request_ptr()
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn ww_reply() -> *mut u8 {
            WW_GUEST.reply_ptr()
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn ww_buf_len() -> i32 {
            WW_GUEST.buf_len()
        }

        #[unsafe(no_mangle)]
        pub extern "C" fn ww_process(len: usize) -> i32 {
            WW_GUEST.process(len)
        }
    };
}

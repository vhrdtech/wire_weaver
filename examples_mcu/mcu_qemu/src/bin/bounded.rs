//! no_std server of an API with bounded lists and strings (`MaxLen`, SW-19): builds on the target and checks at
//! compile time that the largest `write` request fits the server's message buffer.
#![no_main]
#![no_std]

extern crate panic_semihosting;

use core::fmt::Write;
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hio};
use wire_weaver::prelude::*;

/// Size of the request and event buffers.
const MAX_MESSAGE_LEN: usize = 64;

// The worst-case `write` request (offset, last flag and the largest piece) fits the buffer.
const _: () = assert!(matches!(
    api_impl::WRITE_REQUEST_MAX_LEN,
    Some(len) if len <= MAX_MESSAGE_LEN
));

#[entry]
fn main() -> ! {
    let mut stdout = hio::hstdout().unwrap();
    writeln!(
        stdout,
        "write request: at most {:?} bytes",
        api_impl::WRITE_REQUEST_MAX_LEN
    )
    .unwrap();

    let mut scratch = [0u8; MAX_MESSAGE_LEN];
    let mut event_scratch = [0u8; MAX_MESSAGE_LEN];
    let mut server = ServerState {};
    let mut out = EventWriter::new(DummyTx, &mut event_scratch);
    let r = server.process_request_bytes(&[1u8, 2, 3], &mut scratch, &mut out, ());
    writeln!(stdout, "{r:?}").unwrap();

    // exit QEMU
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
    // ---- bounded_api::Bounded at root ----

    /// Stores a piece, returns its length
    fn write(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _offset: u32,
        _last: bool,
        _data: wire_weaver::shrink_wrap::MaxLen<wire_weaver::shrink_wrap::RefVec<'_, u8>, 8>,
    ) -> RpcResult<u8> {
        Unimplemented.into()
    }

    /// Returns `len` bytes of a name, longer than 4 fails to serialize
    fn name(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _len: u8,
    ) -> RpcResult<wire_weaver::shrink_wrap::MaxLen<&'_ str, 4>> {
        Unimplemented.into()
    }

    /// A list of bounded strings, returns how many
    fn tags(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _tags: wire_weaver::shrink_wrap::MaxLen<
            wire_weaver::shrink_wrap::RefVec<'_, wire_weaver::shrink_wrap::MaxLen<&'_ str, 3>>,
            2,
        >,
    ) -> RpcResult<u8> {
        Unimplemented.into()
    }

    /// A struct with a bounded field
    fn piece(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _piece: bounded_api::Piece<'_>,
    ) -> RpcResult<u32> {
        Unimplemented.into()
    }

    /// No bound on the argument
    fn unbounded(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _data: wire_weaver::shrink_wrap::RefVec<'_, u8>,
    ) -> RpcResult<()> {
        Unimplemented.into()
    }

    fn set_label(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _value: wire_weaver::shrink_wrap::MaxLen<&'_ str, 6>,
    ) -> SetResult<()> {
        Unimplemented.into()
    }

    fn get_label(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> GetResult<wire_weaver::shrink_wrap::MaxLen<&'_ str, 6>, ()> {
        Unimplemented.into()
    }

    fn chunks(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
        RpcResult::Ready(())
    }

    fn close_chunks(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

    fn sideband_chunks(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _sideband: ww_client_server::StreamSideband,
    ) -> Option<ww_client_server::StreamSideband> {
        // TODO: None sends no reply
        None
    }

    fn sideband_blocks(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _sideband: ww_client_server::StreamSideband,
    ) -> Option<ww_client_server::StreamSideband> {
        // TODO: None sends no reply
        None
    }

    fn write_blocks(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _data: &wire_weaver::shrink_wrap::tail_bytes::TailBytes<'_>,
    ) {
    }

    /// Sends `len` bytes to `chunks`, returns whether that worked
    fn emit_chunk(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _len: u8,
    ) -> RpcResult<bool> {
        Unimplemented.into()
    }
}

mod api_impl {
    wire_weaver::ww_codegen!(
        bounded_api :: Bounded for super::ServerState,
        server = true, no_alloc = true, use_async = false,
        method_model = "_=immediate",
        property_model = "_=get_set",
    );
}

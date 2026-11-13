//! A server of `flash_budget_api::BaseOnly` (two methods, properties and plain streams) built as a firmware builds
//! it: `ww_client_server` without `std`, so without its `multi` and `streams` features (SRV-60). The test in
//! `tests/left_out.rs` sends it the multi, credit sink and stream sideband requests of the conformance corpus and
//! checks that it refuses them as not supported instead of failing to read them.
//!
//! Its handlers are the stubs of `ww api scaffold tests/flash_budget_api --root BaseOnly`.
#![no_std]

use wire_weaver::prelude::*;

pub struct Server {}

impl Server {
    fn base_c0(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
        RpcResult::Ready(())
    }

    fn base_c1(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
        Unimplemented.into()
    }

    fn base_a0(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _code: flash_budget_api::Code,
    ) -> RpcResult<flash_budget_api::Reading> {
        Unimplemented.into()
    }

    fn base_a1(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _code: flash_budget_api::Code,
    ) -> RpcResult<flash_budget_api::Reading> {
        Unimplemented.into()
    }

    fn set_base_p0(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _value: flash_budget_api::Code,
    ) -> SetResult<()> {
        Unimplemented.into()
    }

    fn get_base_p0(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> GetResult<flash_budget_api::Code, ()> {
        Unimplemented.into()
    }

    fn set_base_p1(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _value: flash_budget_api::Code,
    ) -> SetResult<()> {
        Unimplemented.into()
    }

    fn get_base_p1(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> GetResult<flash_budget_api::Code, ()> {
        Unimplemented.into()
    }

    fn base_s0(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
        RpcResult::Ready(())
    }

    fn close_base_s0(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

    /// Never sees a message this build doesn't know: the generated code refuses those first.
    fn sideband_base_s0(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        sideband: ww_client_server::StreamSideband,
    ) -> Option<ww_client_server::StreamSideband> {
        assert!(!matches!(
            sideband,
            ww_client_server::StreamSideband::Unknown(_)
        ));
        None
    }

    fn base_s1(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
        RpcResult::Ready(())
    }

    fn close_base_s1(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

    fn sideband_base_s1(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _sideband: ww_client_server::StreamSideband,
    ) -> Option<ww_client_server::StreamSideband> {
        None
    }
}

mod api_impl {
    wire_weaver::ww_codegen!(
        flash_budget_api :: BaseOnly for super::Server,
        server = true, no_alloc = true, use_async = false,
        method_model = "_=immediate",
        property_model = "_=get_set",
    );
}

/// Events the handlers send go nowhere: the requests here are all answered directly.
struct NoTx;

impl wire_weaver::BlockingMessageSink for NoTx {
    fn send(&mut self, _message: &[u8]) -> Result<(), ()> {
        Ok(())
    }
}

/// The server's direct reply to `request`, written into `scratch`.
pub fn reply<'a>(request: &[u8], scratch: &'a mut [u8]) -> &'a [u8] {
    let mut event_scratch = [0u8; 256];
    let mut out = EventWriter::new(NoTx, &mut event_scratch);
    Server {}
        .process_request_bytes(request, scratch, &mut out, ())
        .expect("the request decodes")
}

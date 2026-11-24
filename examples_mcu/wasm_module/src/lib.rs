//! The wasm module example: `wasm_module_api`'s `Calc` (`wasm_module_api_evolved`'s with `--features evolved`) as a
//! WireWeaver server compiled to wasm32, exporting the module ABI of `ww_wasm`. The stm32h725ig-wasm-slot firmware
//! runs it in a mount slot.
#![no_std]

use wire_weaver::prelude::*;

pub struct Calc;

impl Calc {
    fn add(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        a: u32,
        b: u32,
    ) -> RpcResult<u32> {
        Ready(a.wrapping_add(b))
    }

    #[cfg(feature = "evolved")]
    fn mul(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        a: u32,
        b: u32,
    ) -> RpcResult<u32> {
        Ready(a.wrapping_mul(b))
    }

    fn trap(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
        trap()
    }

    fn spin(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
        loop {
            core::hint::spin_loop();
        }
    }
}

#[cfg(not(feature = "evolved"))]
mod server_impl {
    wire_weaver::ww_codegen!(
        wasm_module_api :: Calc for super::Calc,
        server = true, no_alloc = true, use_async = false,
        method_model = "_=immediate",
        introspect = "no_docs"
    );
}

#[cfg(feature = "evolved")]
mod server_impl {
    wire_weaver::ww_codegen!(
        wasm_module_api_evolved :: Calc for super::Calc,
        server = true, no_alloc = true, use_async = false,
        method_model = "_=immediate",
        introspect = "no_docs"
    );
}

impl WireWeaverApiBackend for Calc {
    type Medium = ();

    fn process_bytes<'a>(
        &mut self,
        out: &mut impl BlockingEventOut,
        medium: (),
        data: &[u8],
        scratch: &'a mut [u8],
    ) -> Result<&'a [u8], ShrinkWrapError> {
        self.process_request_bytes(data, scratch, out, medium)
    }

    fn version(&self) -> FullVersion<'_> {
        #[cfg(not(feature = "evolved"))]
        return wasm_module_api::CALC_FULL_GID;
        #[cfg(feature = "evolved")]
        return wasm_module_api_evolved::CALC_FULL_GID;
    }
}

ww_wasm::guest_module!(
    Calc = Calc,
    model = server_impl::WW_SELF_BYTES_NO_DOCS,
    buf_len = 512
);

/// A wasm trap: the interpreter stops the module and the slot unloads it.
fn trap() -> ! {
    #[cfg(target_family = "wasm")]
    core::arch::wasm32::unreachable();
    #[cfg(not(target_family = "wasm"))]
    loop {}
}

#[cfg(target_family = "wasm")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    trap()
}

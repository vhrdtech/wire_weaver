//! Shared body of the `qemu-flash-*` bins (TEST-30): a server of one root of `tests/flash_budget_api`, its handlers
//! behind [black_box] so nothing they return is known at compile time, fed one request whose bytes are opaque too,
//! so every arm of the dispatcher stays in the ELF. `examples_mcu/fw_budget --flash` (`just flash-budget`) builds
//! the bins and prints the flash of each against the others.
//!
//! Every handler is `#[inline(never)]` and has a body of its own ([distinct]): identical handlers would let LLVM merge
//! them and the arms that call them, and the budget is the dispatcher's arms, not the handlers.
//!
//! With the `flash-async` feature the server is generated with `use_async = true` and `handler_model = "_=sync"`,
//! as Donguru's is, and `main` polls the request's future once.
//!
//! The panic handler is `panic-semihosting`, which formats the panic message with `core::fmt`, as a firmware's
//! `panic-probe` with `print-defmt` does: a `Debug` impl reachable from a panic lands in flash here as it does
//! there.

#![allow(unused_macros)]

pub use core::hint::black_box;
#[cfg(not(feature = "flash-async"))]
pub use wire_weaver::BlockingEventOut as Out;
#[cfg(feature = "flash-async")]
pub use wire_weaver::EventOut as Out;

/// Makes a handler's body differ from every other's, by a static of its own: LLVM merges identical functions, and
/// then the dispatcher arms that call them.
macro_rules! distinct {
    () => {{
        static ID: u8 = 0;
        core::hint::black_box(&ID);
    }};
}
pub(crate) use distinct;

/// Handlers of calls without arguments or output.
macro_rules! calls {
    ($($name:ident),* $(,)?) => {$(
        #[inline(never)]
        fn $name(&mut self, _cx: &mut Context<'_, impl Out>) -> RpcResult<()> {
            $crate::flash_budget::distinct!();
            $crate::flash_budget::black_box(RpcResult::Ready(()))
        }
    )*};
}

/// Handlers of calls taking a `Code` and returning a `Reading`.
macro_rules! calls_args {
    ($($name:ident),* $(,)?) => {$(
        #[inline(never)]
        fn $name(
            &mut self,
            _cx: &mut Context<'_, impl Out>,
            code: flash_budget_api::Code,
        ) -> RpcResult<flash_budget_api::Reading> {
            let code = $crate::flash_budget::black_box(code);
            $crate::flash_budget::black_box(RpcResult::Ready(flash_budget_api::Reading(code.0 as u32)))
        }
    )*};
}

/// Handlers of calls with arguments and outputs of their own types.
macro_rules! calls_typed {
    ($(($name:ident, $arg:ident, $out:ident)),* $(,)?) => {$(
        #[inline(never)]
        fn $name(
            &mut self,
            _cx: &mut Context<'_, impl Out>,
            arg: flash_budget_api::$arg,
        ) -> RpcResult<flash_budget_api::$out> {
            let arg = $crate::flash_budget::black_box(arg);
            $crate::flash_budget::black_box(RpcResult::Ready(flash_budget_api::$out {
                reading: flash_budget_api::Reading(arg.code.0 as u32),
                code: arg.flag.then_some(arg.code),
            }))
        }
    )*};
}

/// Getters and setters of `Code` properties.
macro_rules! props {
    ($(($get:ident, $set:ident)),* $(,)?) => {$(
        #[inline(never)]
        fn $set(
            &mut self,
            _cx: &mut Context<'_, impl Out>,
            value: flash_budget_api::Code,
        ) -> SetResult<()> {
            $crate::flash_budget::distinct!();
            $crate::flash_budget::black_box(value);
            $crate::flash_budget::black_box(SetResult::Set)
        }

        #[inline(never)]
        fn $get(&mut self, _cx: &mut Context<'_, impl Out>) -> GetResult<flash_budget_api::Code, ()> {
            $crate::flash_budget::distinct!();
            $crate::flash_budget::black_box(GetResult::Value(flash_budget_api::Code(1)))
        }
    )*};
}

/// Getters and setters of `MaxLen<&str, N>` properties, a different bound each (SW-72): every `N` is its own
/// monomorphisation of the bound check, as Donguru's `&str` fields at N = 16, 32, 64, 128 are.
macro_rules! maxlen_props {
    ($(($get:ident, $set:ident, $n:literal)),* $(,)?) => {$(
        #[inline(never)]
        fn $set(
            &mut self,
            _cx: &mut Context<'_, impl Out>,
            value: MaxLen<&'_ str, $n>,
        ) -> SetResult<()> {
            $crate::flash_budget::distinct!();
            $crate::flash_budget::black_box(value);
            $crate::flash_budget::black_box(SetResult::Set)
        }

        #[inline(never)]
        fn $get(&mut self, _cx: &mut Context<'_, impl Out>) -> GetResult<MaxLen<&'_ str, $n>, ()> {
            $crate::flash_budget::distinct!();
            $crate::flash_budget::black_box(GetResult::Value(MaxLen::new_unchecked("x")))
        }
    )*};
}

/// Open, close and sideband handlers of streams.
macro_rules! streams {
    ($(($open:ident, $close:ident, $sideband:ident)),* $(,)?) => {$(
        #[inline(never)]
        fn $open(&mut self, _cx: &mut Context<'_, impl Out>) -> RpcResult<()> {
            $crate::flash_budget::distinct!();
            $crate::flash_budget::black_box(RpcResult::Ready(()))
        }

        #[inline(never)]
        fn $close(&mut self, _cx: &mut Context<'_, impl Out>) {
            $crate::flash_budget::distinct!();
        }

        #[inline(never)]
        fn $sideband(
            &mut self,
            _cx: &mut Context<'_, impl Out>,
            sideband: ww_client_server::StreamSideband,
        ) -> Option<ww_client_server::StreamSideband> {
            $crate::flash_budget::distinct!();
            $crate::flash_budget::black_box(Some(sideband))
        }
    )*};
}

/// The handlers of `flash_budget_api::Base` at `base`.
macro_rules! base_handlers {
    () => {
        calls!(base_c0, base_c1);
        calls_args!(base_a0, base_a1);
        props!((get_base_p0, set_base_p0), (get_base_p1, set_base_p1));
        streams!(
            (base_s0, close_base_s0, sideband_base_s0),
            (base_s1, close_base_s1, sideband_base_s1)
        );
    };
}

/// The bin's `main` and server struct, with `ww_codegen!` for `flash_budget_api::$root`.
macro_rules! flash_budget_server {
    ($root:ident) => {
        use cortex_m_rt::entry;
        use cortex_m_semihosting::debug;

        pub struct ServerState {}

        #[cfg(not(feature = "flash-async"))]
        pub mod api_impl {
            wire_weaver::ww_codegen!(
                flash_budget_api :: $root for super::ServerState,
                server = true, no_alloc = true, use_async = false,
                method_model = "_=immediate",
                property_model = "_=get_set",
                introspect = "no_docs",
            );
        }

        #[cfg(feature = "flash-async")]
        pub mod api_impl {
            wire_weaver::ww_codegen!(
                flash_budget_api :: $root for super::ServerState,
                server = true, no_alloc = true, use_async = true,
                handler_model = "_=sync",
                method_model = "_=immediate",
                property_model = "_=get_set",
                introspect = "no_docs",
            );
        }

        use $crate::flash_budget::Out;

        struct DummyTx;
        #[cfg(not(feature = "flash-async"))]
        impl wire_weaver::BlockingMessageSink for DummyTx {
            fn send(&mut self, message: &[u8]) -> Result<(), ()> {
                $crate::flash_budget::distinct!();
                $crate::flash_budget::black_box(message);
                Ok(())
            }
        }
        #[cfg(feature = "flash-async")]
        impl wire_weaver::MessageSink for DummyTx {
            async fn send(&mut self, message: &[u8]) -> Result<(), ()> {
                $crate::flash_budget::distinct!();
                $crate::flash_budget::black_box(message);
                Ok(())
            }
        }

        /// The server's work, out of `main`, so `main` is the same in every bin.
        #[inline(never)]
        fn serve(request: &[u8]) -> Option<usize> {
            let mut scratch = [0u8; 256];
            let mut event_scratch = [0u8; 256];
            let mut server = ServerState {};
            let mut out = EventWriter::new(DummyTx, &mut event_scratch);
            #[cfg(not(feature = "flash-async"))]
            let r = server.process_request_bytes(request, &mut scratch, &mut out, ());
            #[cfg(feature = "flash-async")]
            let r = {
                let mut fut = core::pin::pin!(server.process_request_bytes(request, &mut scratch, &mut out, ()));
                let mut cx = core::task::Context::from_waker(core::task::Waker::noop());
                match fut.as_mut().poll(&mut cx) {
                    core::task::Poll::Ready(r) => r,
                    core::task::Poll::Pending => Ok(&[][..]),
                }
            };
            r.map(|b| b.len()).ok()
        }

        #[entry]
        fn main() -> ! {
            let mut request = [0u8; 64];
            let request = $crate::flash_budget::black_box(&mut request[..]);
            $crate::flash_budget::black_box(serve(request));
            debug::exit(debug::EXIT_SUCCESS);
            loop {}
        }
    };
}

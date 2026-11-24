//! Client of the wasm slot example firmware (`wasm_slot_api`, `examples_mcu/usb_stm32h725ig`'s
//! `stm32h725ig-wasm-slot`) over USB, and of the modules its slots serve (`wasm_module_api`,
//! `wasm_module_api_evolved`) through a view of a slot. `tests/b135b.rs` runs it against the board.
pub use wire_weaver_client::ClientConfig;
use wire_weaver_client::{Commander, WwClient};

/// The firmware's own API.
pub struct WasmSlot {
    pub cmd: Commander,
}

impl WwClient for WasmSlot {
    fn default_config() -> ClientConfig {
        ClientConfig::new().usb_vid_pid(0xc0de, 0xcafe)
    }

    fn try_from_cmd(cmd: Commander) -> Result<Self, wire_weaver_client::Error> {
        Ok(Self { cmd })
    }
}

mod wasm_slot_client {
    wire_weaver::ww_codegen!(wasm_slot_api :: WasmSlot for crate::WasmSlot, client = "std_client");
}

macro_rules! module_client {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        pub struct $name {
            pub cmd: Commander,
        }

        impl WwClient for $name {
            fn default_config() -> ClientConfig {
                ClientConfig::default()
            }

            fn try_from_cmd(cmd: Commander) -> Result<Self, wire_weaver_client::Error> {
                Ok(Self { cmd })
            }
        }
    };
}

module_client!(
    Calc,
    "A module of `wasm_module_api` 0.1.0, on a view of a slot."
);
module_client!(
    CalcEvolved,
    "A module of `wasm_module_api_evolved` 0.1.1 (with `mul`), on a view of a slot."
);

mod calc_client {
    wire_weaver::ww_codegen!(wasm_module_api :: Calc for crate::Calc, client = "std_client");
}

mod calc_evolved_client {
    wire_weaver::ww_codegen!(wasm_module_api_evolved :: Calc for crate::CalcEvolved, client = "std_client");
}

use wire_weaver::prelude::*;
use wire_weaver_client_common::CommandSender;

pub(crate) struct BankClient {
    #[allow(dead_code)]
    args_scratch: [u8; 64],
    cmd_tx: CommandSender,
}

impl BankClient {
    pub(crate) fn new(cmd_tx: CommandSender) -> Self {
        Self {
            args_scratch: [0u8; 64],
            cmd_tx,
        }
    }
}

pub(crate) struct GpioClient {
    args_scratch: [u8; 64],
    cmd_tx: CommandSender,
}

impl GpioClient {
    pub(crate) fn new(cmd_tx: CommandSender) -> Self {
        Self {
            args_scratch: [0u8; 64],
            cmd_tx,
        }
    }
}

mod bank_client {
    use super::*;
    ww_impl!(
        "../ww_gpio/src/lib.rs" as ww_gpio::GpioBank for BankClient,
        client = "trait_client",
        no_alloc = false,
        use_async = true,
        // debug_to_file = "../target/ww_gpio_hl_bank.rs"
    );
}

mod gpio_client {
    use super::*;
    ww_impl!(
        "../ww_gpio/src/lib.rs" as ww_gpio::Gpio for GpioClient,
        client = "trait_client",
        no_alloc = false,
        use_async = true,
        // debug_to_file = "../target/ww_gpio_hl_gpio.rs"
    );
}

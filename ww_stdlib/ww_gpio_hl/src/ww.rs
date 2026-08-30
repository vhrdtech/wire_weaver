use crate::Error;
use wire_weaver::prelude::*;
use wire_weaver_client::{Attachment, Commander};

#[derive(Clone)]
pub(crate) struct BankClient {
    cmd: Commander,
}

impl BankClient {
    pub(crate) fn new(attachment: Attachment) -> Result<Self, Error> {
        if (attachment.trait_name() != "Bank") || (attachment.source_crate().crate_id != "ww_gpio")
        {
            return Err(Error::IncompatibleTrait(format!(
                "{}::{}",
                attachment.source_crate().crate_id,
                attachment.trait_name()
            )));
        }
        let cmd = attachment.cmd_tx_take();
        Ok(Self { cmd })
    }
}

pub(crate) struct GpioClient {
    cmd: Commander,
}

impl GpioClient {
    pub(crate) fn new(cmd: Commander) -> Self {
        Self { cmd }
    }
}

mod bank_client {
    use super::*;
    ww_impl!(
        ww_gpio :: Bank for BankClient,
        client = "trait_client",
        no_alloc = false,
        use_async = true,
        // debug_to_file = "../../target/ww_gpio_hl_bank.rs"
    );
}

mod gpio_client {
    use super::*;
    ww_impl!(
        ww_gpio :: Pin for GpioClient,
        client = "trait_client",
        no_alloc = false,
        use_async = true,
        // debug_to_file = "../target/ww_gpio_hl_gpio.rs"
    );
}

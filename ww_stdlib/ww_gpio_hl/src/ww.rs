use crate::Error;
use wire_weaver::prelude::*;
use wire_weaver_client_common::{Attachment, CommandSender};

#[derive(Clone)]
pub(crate) struct BankClient {
    cmd_tx: CommandSender,
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
        let cmd_tx = attachment.cmd_tx_take();
        Ok(Self { cmd_tx })
    }
}

pub(crate) struct GpioClient {
    cmd_tx: CommandSender,
}

impl GpioClient {
    pub(crate) fn new(cmd_tx: CommandSender) -> Self {
        Self { cmd_tx }
    }
}

mod bank_client {
    use super::*;
    ww_impl!(
        "../ww_gpio" :: Bank for BankClient,
        client = "trait_client",
        no_alloc = false,
        use_async = true,
        debug_to_file = "../../target/ww_gpio_hl_bank.rs"
    );
}

mod gpio_client {
    use super::*;
    ww_impl!(
        "../ww_gpio" :: Pin for GpioClient,
        client = "trait_client",
        no_alloc = false,
        use_async = true,
        // debug_to_file = "../target/ww_gpio_hl_gpio.rs"
    );
}

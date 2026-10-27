use crate::Error;
use wire_weaver::prelude::*;
use wire_weaver_client::{Attachment, Commander, Priority};

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
    /// The client of one pin and its index in the bank.
    pub(crate) fn from_attachment(gpio_pin: Attachment) -> Result<(Self, u32), Error> {
        if (gpio_pin.trait_name() != "Pin") || (gpio_pin.source_crate().crate_id != "ww_gpio") {
            return Err(Error::IncompatibleTrait(format!(
                "{}::{}",
                gpio_pin.source_crate().crate_id,
                gpio_pin.trait_name()
            )));
        }
        let cmd = gpio_pin.cmd_tx_take();
        let index = if let Some(base_path) = cmd.base_path()
            && let Some(last) = base_path.last()
        {
            last.0
        } else {
            return Err(Error::Internal("empty base path".into()));
        };
        Ok((Self { cmd }, index))
    }

    pub(crate) fn set_priority(&mut self, priority: Priority) {
        self.cmd.set_default_priority(priority);
    }

    pub(crate) fn priority(&self) -> Priority {
        self.cmd.default_priority()
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

use wire_weaver_client_common::{CommandSender, DeviceInfoBundle};
pub use wire_weaver_client_common::{DeviceFilter, OnError};

pub struct AllGpio {
    cmd_tx: CommandSender,
}

impl AllGpio {
    pub fn info(&self) -> &DeviceInfoBundle {
        self.cmd_tx.info()
    }
}

mod api_client {
    wire_weaver::ww_codegen!(
        all_gpio_api :: AllGpioApi for crate::AllGpio,
        client = "async_worker+usb",
        debug_to_file = "../../target/generated_all_gpio_client.rs"
    );
}

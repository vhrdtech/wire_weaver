use wire_weaver_client_common::{CommandSender, DeviceInfoBundle};
pub use wire_weaver_client_common::{DeviceFilter, OnError};

pub struct UartBridge {
    cmd_tx: CommandSender,
}

impl UartBridge {
    pub fn info(&self) -> &DeviceInfoBundle {
        self.cmd_tx.info()
    }
}

mod api_client {
    wire_weaver::ww_codegen!(
        "../uart_api" :: UartBridge for crate::UartBridge,
        client = "async_worker+usb",
        no_alloc = false,
        use_async = true,
        debug_to_file = "../../target/generated_uart_client.rs"
    );
}

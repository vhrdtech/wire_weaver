use std::time::Duration;
use wire_weaver_client_common::{CommandSender, DeviceInfoBundle, Error};
pub use wire_weaver_client_common::{DeviceFilter, OnError};

pub struct UartBridge {
    cmd_tx: CommandSender,
}

impl UartBridge {
    pub async fn connect(filter: DeviceFilter, on_error: OnError) -> Result<Self, Error> {
        UartBridge::connect_raw(
            filter,
            uart_api::UART_BRIDGE_FULL_GID,
            on_error,
            Duration::from_secs(1),
        )
        .await
    }

    pub fn connect_blocking(filter: DeviceFilter, on_error: OnError) -> Result<Self, Error> {
        UartBridge::connect_raw_blocking(
            filter,
            uart_api::UART_BRIDGE_FULL_GID,
            on_error,
            Duration::from_secs(1),
        )
    }

    pub fn info(&self) -> &DeviceInfoBundle {
        self.cmd_tx.info()
    }
}

mod api_client {
    wire_weaver::ww_codegen!(
        "../uart_api" :: UartBridge for UartBridge,
        client = "async_worker+usb",
        no_alloc = false,
        use_async = true,
        debug_to_file = "../../target/generated_uart_client.rs"
    );
}

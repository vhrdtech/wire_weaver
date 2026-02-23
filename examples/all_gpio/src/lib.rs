use std::time::Duration;
use wire_weaver::ww_api;
use wire_weaver_client_common::{CommandSender, DeviceInfoBundle, Error};
pub use wire_weaver_client_common::{DeviceFilter, OnError};

pub struct AllGpio {
    args_scratch: [u8; 4096],
    cmd_tx: CommandSender,
}

impl AllGpio {
    pub async fn connect(filter: DeviceFilter, on_error: OnError) -> Result<Self, Error> {
        AllGpio::connect_raw(
            filter,
            all_gpio_api::ALL_GPIO_API_FULL_GID,
            on_error,
            Duration::from_secs(1),
            [0u8; 4096],
        )
        .await
    }

    pub fn connect_blocking(filter: DeviceFilter, on_error: OnError) -> Result<Self, Error> {
        AllGpio::connect_raw_blocking(
            filter,
            all_gpio_api::ALL_GPIO_API_FULL_GID,
            on_error,
            Duration::from_secs(1),
            [0u8; 4096],
        )
    }

    pub fn info(&self) -> &DeviceInfoBundle {
        self.cmd_tx.info()
    }
}

mod api_client {
    use super::*;
    ww_api!(
        "../all_gpio_api/src/lib.rs" as all_gpio_api::AllGpioApi for AllGpio,
        client = "async_worker+usb",
        no_alloc = true,
        use_async = true,
        debug_to_file = "../../target/generated_all_gpio_client.rs"
    );
}

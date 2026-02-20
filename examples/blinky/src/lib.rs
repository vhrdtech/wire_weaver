use std::time::Duration;
use wire_weaver::ww_api;
use wire_weaver_client_common::{CommandSender, DeviceInfoBundle, Error};
pub use wire_weaver_client_common::{DeviceFilter, OnError};

pub struct Blinky {
    args_scratch: [u8; 4096],
    cmd_tx: CommandSender,
}

impl Blinky {
    pub async fn connect(filter: DeviceFilter, on_error: OnError) -> Result<Self, Error> {
        Blinky::connect_raw(
            filter,
            blinky_api::BLINKY_API_FULL_GID,
            on_error,
            Duration::from_secs(1),
            [0u8; 4096],
        )
        .await
    }

    pub fn connect_blocking(filter: DeviceFilter, on_error: OnError) -> Result<Self, Error> {
        Blinky::connect_raw_blocking(
            filter,
            blinky_api::BLINKY_API_FULL_GID,
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
        "../blinky_api/src/lib.rs" as blinky_api::BlinkyApi for Blinky,
        client = "async_worker+usb",
        no_alloc = true,
        use_async = true,
        debug_to_file = "../../target/generated_blinky_client.rs"
    );
}

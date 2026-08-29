use wire_weaver_client::{CommandSender, DeviceApiInfo};
pub use wire_weaver_client::{DeviceFilter, OnError};

pub struct Blinky {
    cmd_tx: CommandSender,
}

impl Blinky {
    pub fn info(&self) -> &DeviceApiInfo {
        self.cmd_tx.info()
    }
}

mod api_client {
    wire_weaver::ww_codegen!(
        blinky_api :: BlinkyApi for crate::Blinky,
        client = "async_worker+usb",
        debug_to_file = "../../target/generated_blinky_client.rs"
    );
}

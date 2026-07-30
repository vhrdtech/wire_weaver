use wire_weaver_client_common::{CommandSender, DeviceInfoBundle};
pub use wire_weaver_client_common::{DeviceFilter, OnError};

pub struct Blinky {
    cmd_tx: CommandSender,
}

impl Blinky {
    pub fn info(&self) -> &DeviceInfoBundle {
        self.cmd_tx.info()
    }
}

mod api_client {
    wire_weaver::ww_codegen!(
        "../blinky_api_evolved" :: BlinkyApi for crate::Blinky,
        client = "async_worker+usb",
        no_alloc = false,
        use_async = true,
        debug_to_file = "../../target/generated_blinky_evolved_client.rs"
    );
}

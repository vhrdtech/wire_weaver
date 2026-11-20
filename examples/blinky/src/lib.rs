//! Client of the blinky example device (`blinky_api`): the project shape a host program copies, built against
//! USB with `ClientConfig::usb_vid_pid`.
pub use wire_weaver_client::ClientConfig;
use wire_weaver_client::{Commander, DeviceApiInfo, WwClient};

pub struct Blinky {
    cmd: Commander,
}

impl WwClient for Blinky {
    fn default_config() -> ClientConfig {
        ClientConfig::new().usb_vid_pid(0xc0de, 0xcafe)
    }

    fn try_from_cmd(cmd: Commander) -> Result<Self, wire_weaver_client::Error> {
        Ok(Self { cmd })
    }
}

impl Blinky {
    pub fn info(&self) -> &DeviceApiInfo {
        self.cmd.info()
    }
}

mod api_client {
    wire_weaver::ww_codegen!(
        blinky_api :: BlinkyApi for crate::Blinky,
        client = "std_client",
    );
}

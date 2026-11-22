//! Client of the evolved blinky API (`blinky_api_evolved`), run against devices running either version of the
//! API to check that the newer client still works with the older firmware.
use wire_weaver_client::{ClientConfig, Commander, DeviceApiInfo, WwClient};

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
    pub fn info(&self) -> DeviceApiInfo {
        self.cmd.info()
    }
}

mod api_client {
    wire_weaver::ww_codegen!(
        blinky_api_evolved :: BlinkyApi for crate::Blinky,
        client = "std_client",
    );
}

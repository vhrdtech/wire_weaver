//! The blinky example after its first change: [Blinky] is the host side of `blinky_api` 0.1.1 (the
//! `blinky_api_evolved` dependency, with `led_toggle`), and [device] simulates the firmware of 0.1.0 and of 0.1.1.
//! The test `book_output_versions` runs old and new hosts against old and new firmware.

pub mod device;

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

//! Host side of the defmt log example: a client of `defmt_log_api` over USB.

pub use wire_weaver_client::ClientConfig;
use wire_weaver_client::{Commander, WwClient};

pub struct DefmtLog {
    cmd: Commander,
}

impl WwClient for DefmtLog {
    fn default_config() -> ClientConfig {
        ClientConfig::new().usb_vid_pid(0xc0de, 0xcafe)
    }

    fn from_cmd(cmd: Commander) -> Self {
        Self { cmd }
    }
}

mod api_client {
    wire_weaver::ww_codegen!(
        defmt_log_api :: DefmtLogDemo for crate::DefmtLog,
        client = "std_client",
    );
}

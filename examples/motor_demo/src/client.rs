//! The generated client of the demo's API.

use wire_weaver_client::{ClientConfig, Commander, WwClient};

pub struct MotorDemo {
    pub cmd: Commander,
}

impl WwClient for MotorDemo {
    fn default_config() -> ClientConfig {
        ClientConfig::default()
    }

    fn from_cmd(cmd: Commander) -> Self {
        Self { cmd }
    }
}

pub mod api_client {
    wire_weaver::ww_codegen!(
        motor_demo_api :: MotorDemo for super::MotorDemo,
        client = "std_client",
        // debug_to_file = "../../target/motor_demo_client.rs"
    );
}

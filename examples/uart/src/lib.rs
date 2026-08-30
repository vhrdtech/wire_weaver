use wire_weaver_client::{ClientConfig, Commander, DeviceApiInfo, WwClient};

pub struct UartBridge {
    cmd: Commander,
}

impl WwClient for UartBridge {
    fn default_config() -> ClientConfig {
        ClientConfig::new().usb_vid_pid(0xc0de, 0xcafe)
    }

    fn from_cmd(cmd: Commander) -> Self {
        Self { cmd }
    }
}

impl UartBridge {
    pub fn info(&self) -> &DeviceApiInfo {
        self.cmd.info()
    }
}

mod api_client {
    wire_weaver::ww_codegen!(
        uart_api :: UartBridge for crate::UartBridge,
        client = "std_client",
        //debug_to_file = "../../target/generated_uart_client.rs"
    );
}

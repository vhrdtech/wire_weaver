use wire_weaver_client::{ClientConfig, Commander, DeviceApiInfo, WwClient};

pub struct AllGpio {
    cmd: Commander,
}

impl WwClient for AllGpio {
    fn default_config() -> ClientConfig {
        ClientConfig::new().usb_vid_pid(0xc0de, 0xcafe)
    }

    fn from_cmd(cmd: Commander) -> Self {
        Self { cmd }
    }
}

impl AllGpio {
    pub fn info(&self) -> &DeviceApiInfo {
        self.cmd.info()
    }
}

mod api_client {
    wire_weaver::ww_codegen!(
        all_gpio_api :: AllGpioApi for crate::AllGpio,
        client = "std_client",
        //debug_to_file = "../../target/generated_all_gpio_client.rs"
    );
}

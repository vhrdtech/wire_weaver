use ww_self::ApiBundleOwned;

use crate::{ClientConfig, Commander, WwClient};

pub struct DynClient {
    cmd: Commander,
}

impl WwClient for DynClient {
    fn default_config() -> ClientConfig {
        ClientConfig::new()
    }

    fn from_cmd(cmd: Commander) -> Self {
        Self { cmd }
    }
}

use crate::{
    ClientConfig, Commander, DeviceApiInfo, Error, PreparedConnection, WwClient,
    config::IntrospectBundle,
};

/// Dynamic client that can work with any device.
/// Two options are available:
/// - Introspecting a device and then crafting commands in runtime
/// - Using trait calls (e.g., ww_gpio or ww_firmware_info)
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

impl DynClient {
    pub fn new() -> PreparedConnection<Self> {
        PreparedConnection::new(Self::default_config())
    }

    pub fn from_config(config: ClientConfig) -> PreparedConnection<Self> {
        PreparedConnection::new(config)
    }

    pub fn cmd(&self) -> &Commander {
        &self.cmd
    }

    pub fn client_introspect(&self) -> Option<&IntrospectBundle> {
        self.cmd.client_introspect()
    }

    pub fn device_introspect(&self) -> Option<&IntrospectBundle> {
        self.cmd.device_introspect()
    }

    /// Get information about the connected device (API version, link version, max messages size, etc.)
    pub fn device_api_info(&self) -> &DeviceApiInfo {
        self.cmd.device_api_info()
    }

    /// Send disconnect command to a device and wait for it to go through, then stop the even loop and drop all remaining streams or requests.
    pub async fn disconnect(&self) -> Result<(), Error> {
        self.cmd.disconnect().await
    }

    /// Send disconnect command to a device and wait for it to go through, then stop the even loop and drop all remaining streams or requests.
    pub fn disconnect_blocking(&self) -> Result<(), Error> {
        self.cmd.disconnect_blocking()
    }
}

// this will panic due to trying to block a runtime thread
// impl Drop for DynClient {
//     fn drop(&mut self) {
//         _ = self.disconnect_blocking();
//     }
// }

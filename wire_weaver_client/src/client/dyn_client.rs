use crate::{
    ClientConfig, Commander, DeviceApiInfo, PreparedConnection, PreparedDisconnect, WwClient,
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

    pub fn disconnect(&self) -> PreparedDisconnect {
        self.cmd().disconnect()
    }
}

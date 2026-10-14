use crate::{
    Attachment, ClientConfig, Commander, DeviceApiInfo, PreparedConnection, PreparedDisconnect,
    WwClient, config::IntrospectBundle,
};
use ww_version::FullVersion;

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

    /// Connect to any device for global trait calls only, see [ClientConfig::traits_only]. Pass the device
    /// selection in `c`, e.g. `DynClient::traits_only(|c| c.usb().serial_eq(serial))`.
    pub fn traits_only(c: impl Fn(ClientConfig) -> ClientConfig) -> PreparedConnection<Self> {
        PreparedConnection::new(c(Self::default_config().traits_only()))
    }

    /// Attachment for a global trait wherever the device implements it: calls through it carry a global trait path
    /// (`gid` and `trait_name`, e.g. `ww_dfu::BOOT_CONTROL_FULL_GID`, `"BootControl"`) that the device resolves
    /// (API-20). A device that doesn't implement a compatible version of the trait answers with
    /// `ww_client_server::ErrorKind::TraitNotFound`.
    pub fn global_attachment(&self, gid: FullVersion<'_>, trait_name: &str) -> Attachment {
        Attachment::new(self.cmd.clone(), gid.make_owned(), trait_name.to_string())
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

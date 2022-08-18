use crate::serdes::vlu4::{SemVer};

#[derive(Copy, Clone, Debug)]
pub struct NodeInfo<'info> {
    /// User friendly name of the node, maybe changeable through it's xPI
    pub friendly_name: &'info str,
    /// Information about the underlying platform this node is running on
    pub running_on: PlatformInfo,
    /// UUID of the node, shouldn't change during operation, may change on reboot or can be fixed in firmware
    pub uuid: u128,
    /// Unique id of the project in [vhL Registry](https://www.notion.so/vhrdtech/vhL-Registry-5799542cf9dd41b0a92c702aa05f8c42).
    /// Node must implement and follow vhL sources of the exact version published
    pub vhl_registry_id: u32,
    /// Version of the project in the Registry.
    pub vhl_version: SemVer,
}

#[derive(Copy, Clone, Debug)]
pub enum PlatformInfo {
    Mcu {
        // series, core, hw_info (name, revision, variant), firmware_info (name, features, version, repo+sha, crc, size, signature)
    },
    Wasm {
        // running_on: PlatformInfo,
        // vm_info:
    },
    Mac,
    Linux,
    Windows,
    Ios,
    Android,
    Web,
    Other
}

/// Distributed periodically by all active nodes
/// Counter resetting means device has rebooted and all active subscriptions to it must be re-done.
/// Overflow over u32::MAX doesn't count.
///
/// More specific node status and information might be made available through it's specific xPI.
///
/// CAN Bus note: should be possible to encode more data into the same frame for more specific info.
/// So that resources are preserved. Expose it through node's own xPI.
#[derive(Copy, Clone, Debug)]
pub struct HeartbeatInfo {
    pub health: NodeHealthStatus,
    pub uptime_seconds: u32,
}

#[derive(Copy, Clone, Debug)]
pub enum NodeHealthStatus {
    /// Fully functioning node
    Norminal,
    /// Node can perform it's task, but is experiencing troubles
    Warning,
    /// Node cannot perform it's task
    Failure
}


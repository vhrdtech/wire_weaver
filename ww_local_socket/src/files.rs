//! Names of the files in [runtime_dir](crate::runtime_dir) that the hub and its clients agree on.

/// The hub's socket (on Windows the pipe named after it).
pub const HUB_SOCKET: &str = "hub.sock";
/// Held exclusively by the running hub for its whole life (an OS file lock, released on a crash): one hub per user,
/// and a socket file without it held is stale.
pub const HUB_LOCK: &str = "hub.lock";
/// Lock files of the devices' holders (CLT-38), one per device key.
pub const HOLDERS_DIR: &str = "holders";
/// Held by whoever starts a hub, so two programs starting one at once end up with one.
pub const HUB_START_LOCK: &str = "hub.start.lock";
/// The open counter of each device (CLT-39): when it was opened lately, one file per device key.
pub const OPENS_DIR: &str = "opens";
/// Where a hub started by a client writes its output (stderr), replaced at each start.
pub const HUB_LOG: &str = "hub.log";

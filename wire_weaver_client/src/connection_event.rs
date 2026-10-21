//! What happens to a client's connection (CLT-40): the link went, and, for a connection through the ww hub, the
//! client connected again by itself. From [Commander::connection_events](crate::Commander::connection_events).

use std::time::Duration;

use wire_weaver::DisconnectReason;

/// `DisconnectReason::Other(DISCONNECT_LENT)`: the hub closes a tunnel to a device it lent to another program
/// (SRV-32, `ww_local_socket::DISCONNECT_LENT`).
const DISCONNECT_LENT: u8 = 1;
/// Preamble answer status of a hub that lent the device to another program (SRV-32, `ww_local_socket::Status::Lent`).
pub(crate) const STATUS_LENT: u8 = 5;

/// Why a connection has a gap.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum GapReason {
    /// The link broke: the medium ended (the hub exited, crashed or was killed; a device was unplugged), an IO
    /// error, or nothing came within the peer timeout.
    Lost(String),
    /// The hub handed over to a newer one (SRV-30): the device stays open in the new hub.
    HubHandover,
    /// The hub lent the device to another program for a while (SRV-32): it comes back when that one is done.
    Lent,
    /// The other side ended the link for this reason.
    Disconnect(DisconnectReason),
}

impl GapReason {
    pub(crate) fn from_disconnect(reason: DisconnectReason) -> Self {
        match reason {
            DisconnectReason::HubHandover => GapReason::HubHandover,
            DisconnectReason::Other(DISCONNECT_LENT) => GapReason::Lent,
            other => GapReason::Disconnect(other),
        }
    }
}

impl std::fmt::Display for GapReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GapReason::Lost(why) => write!(f, "link lost: {why}"),
            GapReason::HubHandover => f.write_str("the ww hub handed over to a newer one"),
            GapReason::Lent => f.write_str("the ww hub lent the device to another program"),
            GapReason::Disconnect(reason) => {
                write!(f, "disconnected by the other side: {reason:?}")
            }
        }
    }
}

/// What happened to the connection, see [Commander::connection_events](crate::Commander::connection_events).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConnectionEvent {
    /// The link went. Calls in flight failed with [Error::Disconnected](crate::Error::Disconnected), and so does
    /// every call until a reconnect (they are never sent again by themselves). With `reconnecting`, the client
    /// connects again by itself (a connection through the hub, [ClientConfig::hub_reconnect]
    /// (crate::ClientConfig::hub_reconnect)): counted and reliable streams, observed levels and sinks wait for it.
    Disconnected {
        reason: GapReason,
        reconnecting: bool,
    },
    /// A reconnect attempt failed; the next comes after `retry_in`.
    AttemptFailed {
        attempt: u32,
        error: String,
        retry_in: Duration,
    },
    /// Connected again after a gap of `gap`: link setup is done and `resumed` counted and reliable streams,
    /// observed levels and credit sink sessions go on (their handles see `Resumed`, with gaps if the device didn't
    /// keep everything).
    Reconnected {
        gap: Duration,
        attempts: u32,
        resumed: usize,
    },
    /// The client stopped trying: no reconnect within the give-up time, a device with another API, versions the hub
    /// doesn't serve. The waiting streams end; connect a new client to go on.
    GaveUp { error: String },
}

//! What happens to a client's connection (CLT-40, CLT-75): the link went, and, for a connection through the ww hub or
//! a keep-trying one, the client connected again by itself. From
//! [Commander::connection_events](crate::Commander::connection_events) and
//! [Commander::connection_state](crate::Commander::connection_state).

use std::time::Duration;

use wire_weaver::DisconnectReason;

/// `DisconnectReason::Other(DISCONNECT_LENT)`: the hub closes a tunnel to a device it lent to another program
/// (SRV-32, `ww_byte_stream::DISCONNECT_LENT`).
const DISCONNECT_LENT: u8 = 1;
/// Preamble answer status of a hub that lent the device to another program (SRV-32, `ww_byte_stream::Status::Lent`).
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
    /// Another program asked for the device this program held directly, and it handed the device to the ww hub
    /// (CLT-43, [ClientConfig::allow_handover](crate::ClientConfig::allow_handover)): it reconnects through the hub.
    HandedToHub,
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
            GapReason::HandedToHub => {
                f.write_str("handed the device to the ww hub for another program")
            }
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
    /// A keep-trying connection (CLT-75) starts attempt `attempt` (1 for the first since the link went, or since the
    /// connection was made), racing its addresses (CLT-80).
    Connecting { attempt: u32 },
    /// A keep-trying connection (CLT-75) connected for the first time, at `address` (as `DeviceInfo::location` names
    /// it), after `attempts` attempts. Later connects are [Self::Reconnected].
    Connected { address: String, attempts: u32 },
    /// The link went. Calls in flight failed with [Error::Disconnected](crate::Error::Disconnected), and so does
    /// every call until a reconnect (they are never sent again by themselves). With `reconnecting`, the client
    /// connects again by itself (a connection through the hub, [crate::ClientConfig::hub_reconnect], or one handed to
    /// the hub, [crate::ClientConfig::allow_handover]): counted and
    /// reliable streams, observed levels and sinks wait for it.
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

/// Where a connection is now, see [Commander::connection_state](crate::Commander::connection_state): what
/// [ConnectionEvent]s told so far, for an app that shows it rather than following every event.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConnectionState {
    /// Not connected, and trying: attempt `attempt` runs or waits for its turn (0 before the first one).
    Connecting { attempt: u32 },
    /// Connected to the device at `address` (as `DeviceInfo::location` names it).
    Connected { address: String },
    /// Not connected, and not trying: the link went on a connection that doesn't reconnect by itself, `disconnect()`
    /// was asked for, or the client gave up ([ConnectionEvent::GaveUp]).
    Disconnected,
}

/// What [Commander::retry_now](crate::Commander::retry_now) did (CLT-75), by where the connection was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RetryNow {
    /// Waiting between attempts: the next one starts now, and the wait after it is the backoff's first again.
    Attempting,
    /// An attempt was under way: it was stopped and a new one starts now, from the first address.
    Restarted,
    /// Connected: a ping went to the device. A link whose other end is gone shows up as an error of the medium when
    /// its OS knows (a reset TCP connection), otherwise at the peer timeout (10 s without a word from the device),
    /// and then reconnects as any lost link does.
    Pinged,
    /// The connection doesn't reconnect by itself, so there is nothing to hurry: a plain `connect()` (unless through
    /// the ww hub), `connect_blocking()`, one stopped by `disconnect()`, or one that gave up. Nothing was done.
    NotTrying,
}

/// The numbers above are `ww_local_socket`'s, which the client links only with the `local_socket` feature.
#[cfg(all(test, feature = "local_socket"))]
mod same_numbers {
    #[test]
    fn lent_numbers_are_the_hubs() {
        assert_eq!(super::DISCONNECT_LENT, ww_byte_stream::DISCONNECT_LENT);
        assert_eq!(super::STATUS_LENT, ww_byte_stream::Status::Lent.to_u8());
    }
}

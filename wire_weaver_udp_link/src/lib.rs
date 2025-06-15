use wire_weaver::prelude::*;
use ww_version::FullVersion;

/// UDP datagram data structure supporting efficient transport of one or more ww_client_server Request's or Event's.
/// In addition there are version checks in place, maximum datagram length handshake, provisions for backwards compatibility
/// and expansion of the protocol.
///
/// Each datagram is carrying one or more Op's to conserve bandwidth and/or latency.
/// If Op's are accumulated for a longer period, better bandwidth utilization can be achieved.
/// On the other hand if there are too many small events, packing them all into one datagram allows to get more of them
/// across, without network stack limitations or overflows of some kind.
#[derive_shrink_wrap]
#[shrink_wrap(no_alloc)]
struct Datagram<'i> {
    /// Constant 0xDA7A_63A1 to filter out stray datagrams and also encode this protocol version (and wire_weaver/shrink_wrap version as well).
    /// Assuming that if someones wants to craft a malicious datagram they can still do it, even if SHA256 or such is used,
    /// and a constant requires no compute.
    magic: u32,
    /// Monotonically increasing number, wrapping to zero.
    /// Used to discard repeated datagrams. Can also be used to re-arrange out of order datagrams if need be in the future.
    seq: u16,
    /// One or more Op. Empty vector should not be sent, but just in case it is ignored on reception.
    ops: RefVec<'i, Op<'i>>,
}

pub const UDP_LINK_MAGIC: u32 = 0xDA7A_63A1;

#[derive_shrink_wrap]
#[ww_repr(u4)]
#[shrink_wrap(no_alloc)]
#[derive(Clone, Debug, Eq, PartialEq)]
enum Op<'i> {
    /// ww_client_server serialized Request
    RequestData { data: RefVec<'i, u8> },
    /// ww_client_server serialized Event
    EventData { data: RefVec<'i, u8> },

    /// Sent from client to server
    GetDeviceInfo,
    /// Answer to GetDeviceInfo from server to client
    DeviceInfo {
        /// Server side ww_client_server version
        server: FullVersion<'i>,
        /// Server side user API version
        user: FullVersion<'i>,
        /// Maximum datagram length that server can receive
        max_datagram_length: u16,
    },

    /// Sent from client to server
    LinkSetup {
        /// Client side ww_client_server version
        client: FullVersion<'i>,
        /// Client side user API version
        user: FullVersion<'i>,
        /// Maximum datagram length that client can receive
        max_datagram_length: u16,
    },
    /// Answer to LinkSetup from server to client
    LinkSetupResult { is_compatible: bool },

    /// Periodically sent from client to server, server will consider client disconnected after timeout,
    /// and stop sending data (for example if client crashed or closed without sending Disconnect for any reason).
    KeepAlive,
    /// Sent from client or server when it is about to disconnect.
    Disconnect { reason: &'i str },
}

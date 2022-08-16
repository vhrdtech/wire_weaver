use crate::serdes::xpi_vlu4::node_info::{HeartbeatInfo, NodeInfo};
use crate::serdes::xpi_vlu4::NodeId;

pub struct XpiBroadcast<'br> {
    pub source: NodeId,
    pub kind: XpiBroadcastKind<'br>
}
/// Bidirectional functionality of the Link. Node discovery and heartbeats.
/// Self node id
/// No request id is sent or received for XpiMulti
#[derive(Copy, Clone, Debug)]
pub enum XpiBroadcastKind<'br> {
    /// Broadcast request to all the nodes to announce themselves.
    /// Up to the user how to actually implement this (for example zeroconf or randomly
    /// delayed transmissions on CAN Bus if unique IDs wasn't assigned yet).
    DiscoverNodes,
    /// Sent by nodes in response to [XpiRequest::DiscoverNodes]. Received by everyone else.
    NodeInfo(NodeInfo<'br>),
    /// Sent by all nodes periodically, received by all nodes.
    /// Must be sent with maximum lossy priority.
    /// If emergency stop messages exist in a system, heartbeats should be sent with the next lower priority.
    Heartbeat(HeartbeatInfo),
}

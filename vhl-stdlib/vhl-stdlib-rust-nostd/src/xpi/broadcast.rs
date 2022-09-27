/// Bidirectional functionality of the Link. Node discovery and heartbeats.
#[derive(Copy, Clone, Debug)]
pub enum XpiGenericBroadcastKind<
    N, // Node info
    H, // Heartbeat info
> {
    /// Broadcast request to all the nodes to announce themselves.
    /// Up to the user how to actually implement this (for example zeroconf or randomly
    /// delayed transmissions on CAN Bus if unique IDs wasn't assigned yet).
    DiscoverNodes,
    /// Sent by nodes in response to [XpiRequest::DiscoverNodes]. Received by everyone else.
    NodeInfo(N),
    /// Sent by all nodes periodically, received by all nodes.
    /// Must be sent with maximum lossy priority.
    /// If emergency stop messages exist in a system, heartbeats should be sent with the next lower priority.
    Heartbeat(H),
}
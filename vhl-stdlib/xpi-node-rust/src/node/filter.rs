use xpi::owned::NodeId;

pub enum SourceFilter {
    Any,
    NodeId(NodeId),
}

pub enum NodeSetFilter {
    Any,
    NodeId(NodeId),
    UnicastTraits,
    Multicast,
    Broadcast,
}


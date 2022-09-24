use crate::serdes::xpi_vlu4::node_info::{HeartbeatInfo, NodeInfo};
use crate::serdes::xpi_vlu4::NodeId;

pub struct XpiBroadcast<'br> {
    pub source: NodeId,
    pub kind: XpiBroadcastKind<'br>
}


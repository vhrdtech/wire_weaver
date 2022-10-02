use std::fmt::{Display, Formatter};
use crate::node_set::XpiGenericNodeSet;
use crate::owned::convert_error::ConvertError;
use crate::owned::node_id::NodeId;
use crate::owned::trait_set::TraitSet;
use crate::xwfd;
use vhl_stdlib::serdes::{BitBufMut, NibbleBufMut};

pub type NodeSet = XpiGenericNodeSet<NodeId, TraitSet>;

impl NodeSet {
    pub(crate) fn ser_header_xwfd(&self, bwr: &mut BitBufMut) -> Result<(), ConvertError> {
        match &self {
            NodeSet::Unicast(node_id) => {
                bwr.put_up_to_8(2, 0b00)?;
                let node_id: xwfd::NodeId = node_id.clone().try_into()?;
                bwr.put(&node_id)?;
            }
            NodeSet::UnicastTraits { .. } => unimplemented!(),
            NodeSet::Multicast { .. } => unimplemented!(),
            NodeSet::Broadcast { original_source } => {
                bwr.put_up_to_8(2, 0b11)?;
                let node_id: xwfd::NodeId = original_source.clone().try_into()?;
                bwr.put(&node_id)?;
            }
        }
        Ok(())
    }

    pub(crate) fn ser_body_xwfd(&self, _nwr: &mut NibbleBufMut) -> Result<(), ConvertError> {
        match &self {
            NodeSet::Unicast(_) => {} // no need for additional data
            NodeSet::UnicastTraits { .. } => unimplemented!(),
            NodeSet::Multicast { .. } => unimplemented!(),
            NodeSet::Broadcast { .. } => {} // no need for additional data
        }
        Ok(())
    }
}

impl<'i> From<xwfd::NodeSet<'i>> for NodeSet {
    fn from(node_set: xwfd::NodeSet<'i>) -> Self {
        match node_set {
            xwfd::NodeSet::Unicast(dst) => NodeSet::Unicast(dst.into()),
            xwfd::NodeSet::UnicastTraits { .. } => unimplemented!(),
            xwfd::NodeSet::Multicast { .. } => unimplemented!(),
            xwfd::NodeSet::Broadcast { original_source } => NodeSet::Broadcast { original_source: original_source.into() }
        }
    }
}

impl Display for NodeSet {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeSet::Unicast(dst) => write!(f, "U_{}", dst),
            NodeSet::UnicastTraits { .. } => write!(f, "impl"),
            NodeSet::Multicast { .. } => write!(f, "multi"),
            NodeSet::Broadcast { .. } => write!(f, "*")
        }
    }
}
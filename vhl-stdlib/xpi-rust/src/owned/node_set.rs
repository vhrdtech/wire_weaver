use crate::node_set::XpiGenericNodeSet;
use crate::owned::convert_error::ConvertError;
use crate::owned::node_id::NodeId;
use crate::owned::trait_set::TraitSet;
use crate::xwfd;
use vhl_stdlib_nostd::serdes::{BitBufMut, NibbleBufMut};

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
            NodeSet::Broadcast => {
                bwr.put_up_to_8(2, 0b11)?;
            }
        }
        Ok(())
    }

    pub(crate) fn ser_body_xwfd(&self, _nwr: &mut NibbleBufMut) -> Result<(), ConvertError> {
        match &self {
            NodeSet::Unicast(_) => {} // no need for additional data
            NodeSet::UnicastTraits { .. } => unimplemented!(),
            NodeSet::Multicast { .. } => unimplemented!(),
            NodeSet::Broadcast => {} // no need for additional data
        }
        Ok(())
    }
}
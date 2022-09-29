use vhl_stdlib_nostd::serdes::BitBufMut;
use crate::node_set::XpiGenericNodeSet;
use crate::owned::node_id::NodeId;
use crate::owned::convert_error::ConvertError;
use crate::owned::trait_set::TraitSet;
use crate::xwfd;

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
}

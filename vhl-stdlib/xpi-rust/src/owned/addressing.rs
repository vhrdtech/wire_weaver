use vhl_stdlib_nostd::serdes::BitBufMut;
use crate::node_set::XpiGenericNodeSet;
use crate::owned::error::ConvertError;
use crate::resource_set::XpiGenericResourceSet;
use crate::xwfd;
use super::{SerialMultiUri, SerialUri};

#[derive(Copy, Clone)]
pub struct RequestId(pub u32);

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct NodeId(pub u32);

impl TryInto<xwfd::NodeId> for NodeId {
    type Error = ConvertError;

    fn try_into(self) -> Result<xwfd::NodeId, Self::Error> {
        if self.0 <= 127 {
            Ok(unsafe { xwfd::NodeId::new_unchecked(self.0 as u8) })
        } else {
            Err(ConvertError::NodeIdTruncate)
        }
    }
}


pub type XpiResourceSet = XpiGenericResourceSet<SerialUri, SerialMultiUri>;

impl XpiResourceSet {
    pub(crate) fn ser_header_xwfd(&self, bwr: &mut BitBufMut) -> Result<(), ConvertError> {
        match &self {
            XpiGenericResourceSet::Uri(uri) => {}
            XpiGenericResourceSet::MultiUri(multi_uri) => {}
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct TraitSet {}

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
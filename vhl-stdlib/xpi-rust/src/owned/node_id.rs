use crate::owned::convert_error::ConvertError;
use crate::xwfd;

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

impl From<xwfd::NodeId> for NodeId {
    fn from(id: xwfd::NodeId) -> Self {
        NodeId(id.inner() as u32)
    }
}
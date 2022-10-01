use crate::owned::convert_error::ConvertError;
use crate::xwfd;

#[derive(Copy, Clone)]
pub struct RequestId(pub u32);

impl TryInto<xwfd::RequestId> for RequestId {
    type Error = ConvertError;

    fn try_into(self) -> Result<xwfd::RequestId, Self::Error> {
        if self.0 <= 31 {
            Ok(unsafe { xwfd::RequestId::new_unchecked(self.0 as u8) })
        } else {
            Err(ConvertError::RequestIdTruncated)
        }
    }
}

impl From<xwfd::RequestId> for RequestId {
    fn from(id: xwfd::RequestId) -> Self {
        RequestId(id.inner() as u32)
    }
}
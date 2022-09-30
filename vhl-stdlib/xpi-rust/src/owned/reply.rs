use vhl_stdlib_nostd::serdes::{BitBufMut, NibbleBufMut};
use crate::reply::{XpiGenericReply, XpiGenericReplyKind};
use crate::error::XpiError;
use crate::owned::convert_error::ConvertError;
use crate::owned::request_id::RequestId;
use super::{ResourceInfo, SerialMultiUri, SerialUri};

/// Owned XpiReply relying on allocators and std
/// See [XpiGenericReply](crate::xpi::request::XpiGenericReply) for detailed information.
pub type Reply = XpiGenericReply<
    SerialUri,
    SerialMultiUri,
    Vec<Vec<u8>>,
    Vec<Result<Vec<u8>, XpiError>>,
    Vec<Result<(), XpiError>>,
    Vec<Result<(), ResourceInfo>>,
    RequestId
>;

/// See [XpiGenericReplyKind](crate::xpi::request::XpiGenericReplyKind) for detailed information.
pub type ReplyKind = XpiGenericReplyKind<
    Vec<Vec<u8>>,
    Vec<Result<Vec<u8>, XpiError>>,
    Vec<Result<(), XpiError>>,
    Vec<Result<(), ResourceInfo>>,
>;

impl ReplyKind {
    pub(crate) fn ser_header_xwfd(&self, bwr: &mut BitBufMut) -> Result<(), ConvertError> {
        bwr.put_up_to_8(4, self.discriminant() as u8)?;
        Ok(())
    }

    pub(crate) fn ser_body_xwfd(&self, _nwr: &mut NibbleBufMut) -> Result<(), ConvertError> {
        Ok(())
    }
}
use super::{ResourceInfo, SerialMultiUri, SerialUri};
use crate::error::XpiError;
use crate::owned::convert_error::ConvertError;
use crate::owned::request_id::RequestId;
use crate::reply::{XpiGenericReply, XpiGenericReplyKind};
use crate::xwfd;
use vhl_stdlib::serdes::{BitBufMut, NibbleBufMut};

/// Owned XpiReply relying on allocators and std
/// See [XpiGenericReply](crate::xpi::request::XpiGenericReply) for detailed information.
pub type Reply = XpiGenericReply<
    SerialUri,
    SerialMultiUri,
    Vec<Vec<u8>>,
    Vec<Result<Vec<u8>, XpiError>>,
    Vec<Result<(), XpiError>>,
    Vec<Result<(), ResourceInfo>>,
    RequestId,
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

    pub(crate) fn ser_body_xwfd(&self, nwr: &mut NibbleBufMut) -> Result<(), ConvertError> {
        match self {
            ReplyKind::CallComplete(results) => {
                nwr.put_vec_with(|vb| results.iter().try_for_each(|result| vb.put(result)))?;
            }
            _ => unimplemented!(), // ReplyKind::ReadComplete(_) => {}
            // ReplyKind::WriteComplete(_) => {}
            // ReplyKind::OpenStream(_) => {}
            // ReplyKind::StreamUpdate(_) => {}
            // ReplyKind::CloseStream(_) => {}
            // ReplyKind::Subscribe(_) => {}
            // ReplyKind::RateChange(_) => {}
            // ReplyKind::Unsubscribe(_) => {}
            // ReplyKind::Borrow(_) => {}
            // ReplyKind::Release(_) => {}
            // ReplyKind::Introspect(_) => {}
        }
        Ok(())
    }
}

impl<'i> From<xwfd::Reply<'i>> for Reply {
    fn from(req: xwfd::Reply<'i>) -> Self {
        Reply {
            resource_set: req.resource_set.into(),
            kind: req.kind.into(),
            request_id: req.request_id.into(),
        }
    }
}

impl<'i> From<xwfd::ReplyKind<'i>> for ReplyKind {
    fn from(rep: xwfd::ReplyKind<'i>) -> Self {
        match rep {
            xwfd::ReplyKind::CallComplete(results) => ReplyKind::CallComplete(
                results
                    .iter()
                    .map(|r| r.map(|slice| slice.to_owned()))
                    .collect(),
            ),
            _ => unimplemented!(),
            // xwfd::ReplyKind::ReadComplete(_) => {}
            // xwfd::ReplyKind::WriteComplete(_) => {}
            // xwfd::ReplyKind::OpenStream(_) => {}
            // xwfd::ReplyKind::StreamUpdate(_) => {}
            // xwfd::ReplyKind::CloseStream(_) => {}
            // xwfd::ReplyKind::Subscribe(_) => {}
            // xwfd::ReplyKind::RateChange(_) => {}
            // xwfd::ReplyKind::Unsubscribe(_) => {}
            // xwfd::ReplyKind::Borrow(_) => {}
            // xwfd::ReplyKind::Release(_) => {}
            // xwfd::ReplyKind::Introspect(_) => {}
        }
    }
}

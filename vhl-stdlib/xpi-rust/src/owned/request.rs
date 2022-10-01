use vhl_stdlib::serdes::{BitBufMut, NibbleBufMut};
use crate::owned::convert_error::ConvertError;
use crate::owned::rate::Rate;
use crate::owned::request_id::RequestId;
use crate::request::{XpiGenericRequest, XpiGenericRequestKind};
use super::{SerialMultiUri, SerialUri};

/// Owned XpiRequest relying on allocators and std
/// See [XpiGenericRequest](crate::xpi::request::XpiGenericRequest) for detailed information.
pub type Request = XpiGenericRequest<
    SerialUri,
    SerialMultiUri,
    Vec<u8>,
    Vec<Vec<u8>>,
    Vec<Rate>,
    RequestId,
>;

/// See [XpiGenericRequestKind](crate::xpi::request::XpiGenericRequestKind) for detailed information.
pub type RequestKind = XpiGenericRequestKind<
    Vec<u8>,
    Vec<Vec<u8>>,
    Vec<Rate>
>;

impl RequestKind {
    pub(crate) fn ser_header_xwfd(&self, bwr: &mut BitBufMut) -> Result<(), ConvertError> {
        bwr.put_up_to_8(4, self.discriminant() as u8)?;
        Ok(())
    }

    pub(crate) fn ser_body_xwfd(&self, nwr: &mut NibbleBufMut) -> Result<(), ConvertError> {
        match self {
            RequestKind::Call { args_set } => {
                nwr.put_vec_with(|vb| {
                    args_set.iter().try_for_each(|args| vb.put(&args.as_slice()))
                })?;
            }
            _ => unimplemented!()
            // RequestKind::ChainCall { .. } => {}
            // RequestKind::Read => {}
            // RequestKind::Write { .. } => {}
            // RequestKind::OpenStreams => {}
            // RequestKind::CloseStreams => {}
            // RequestKind::Subscribe { .. } => {}
            // RequestKind::Unsubscribe => {}
            // RequestKind::Borrow => {}
            // RequestKind::Release => {}
            // RequestKind::Introspect => {}
        }
        Ok(())
    }
}
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
pub type RequestKind<'req> = XpiGenericRequestKind<
    Vec<u8>,
    Vec<Vec<u8>>,
    Vec<Rate>
>;
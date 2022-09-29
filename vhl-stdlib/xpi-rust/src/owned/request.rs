use crate::request::{XpiGenericRequest, XpiGenericRequestKind};
use super::{SerialUri, SerialMultiUri, Rate, RequestId};

/// Owned XpiRequest relying on allocators and std
/// See [XpiGenericRequest](crate::xpi::request::XpiGenericRequest) for detailed information.
pub type XpiRequest = XpiGenericRequest<
    SerialUri,
    SerialMultiUri,
    Vec<u8>,
    Vec<Vec<u8>>,
    Vec<Rate>,
    RequestId,
>;

/// See [XpiGenericRequestKind](crate::xpi::request::XpiGenericRequestKind) for detailed information.
pub type XpiRequestKind<'req> = XpiGenericRequestKind<
    Vec<u8>,
    Vec<Vec<u8>>,
    Vec<Rate>
>;
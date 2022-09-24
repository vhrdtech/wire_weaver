use crate::xpi::addressing::XpiGenericResourceSet;
use crate::xpi::request::{XpiGenericRequest, XpiGenericRequestKind};

// pub type XpiResourceSet = XpiGenericResourceSet<Uri, MultiUri>;

// /// Owned XpiRequest relying on allocators and std
// /// See [XpiGenericRequest](crate::xpi::request::XpiGenericRequest) for detailed information.
// pub type XpiRequest = XpiGenericRequest<
//     XpiResourceSet<'req>,
//     Vec<u8>,
//     Vec<Vec<u8>>,
//     Vlu4Vec<'req, Rate>,
//     RequestId,
// >;
//
// /// See [XpiGenericRequestKind](crate::xpi::request::XpiGenericRequestKind) for detailed information.
// pub type XpiRequestKind<'req> = XpiGenericRequestKind<
//     Vec<u8>,
//     Vec<Vec<u8>>,
//     Vlu4Vec<'req, Rate>
// >;
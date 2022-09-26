use crate::serdes::xpi_vlu4::error::FailReason;
use crate::xpi::reply::XpiGenericReply;
use super::{SerialUri, SerialMultiUri, ResourceInfo, RequestId};

pub type XpiReply = XpiGenericReply<
    SerialUri,
    SerialMultiUri,
    Vec<Vec<u8>>,
    Vec<Result<Vec<u8>, FailReason>>,
    Vec<Result<(), FailReason>>,
    Vec<Result<(), ResourceInfo>>,
    RequestId
>;
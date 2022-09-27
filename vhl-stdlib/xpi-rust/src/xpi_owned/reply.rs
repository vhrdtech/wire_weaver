use crate::reply::XpiGenericReply;
use crate::xpi_vlu4::error::FailReason;
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
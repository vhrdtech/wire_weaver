use crate::reply::XpiGenericReply;
use crate::error::XpiError;
use super::{SerialUri, SerialMultiUri, ResourceInfo, RequestId};

pub type XpiReply = XpiGenericReply<
    SerialUri,
    SerialMultiUri,
    Vec<Vec<u8>>,
    Vec<Result<Vec<u8>, XpiError>>,
    Vec<Result<(), XpiError>>,
    Vec<Result<(), ResourceInfo>>,
    RequestId
>;
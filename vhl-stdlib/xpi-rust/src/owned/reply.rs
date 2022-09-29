use crate::reply::XpiGenericReply;
use crate::error::XpiError;
use crate::owned::request_id::RequestId;
use super::{ResourceInfo, SerialMultiUri, SerialUri};

pub type Reply = XpiGenericReply<
    SerialUri,
    SerialMultiUri,
    Vec<Vec<u8>>,
    Vec<Result<Vec<u8>, XpiError>>,
    Vec<Result<(), XpiError>>,
    Vec<Result<(), ResourceInfo>>,
    RequestId
>;
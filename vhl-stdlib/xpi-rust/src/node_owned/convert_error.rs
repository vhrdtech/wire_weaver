use crate::error::XpiError;
use vhl_stdlib::serdes::{bit_buf, nibble_buf};

#[derive(Debug)]
pub enum ConvertError {
    BitBuf(bit_buf::Error),
    NibbleBuf(nibble_buf::Error),
    NodeIdTruncate,
    PriorityTruncate,
    XpiError(XpiError),
    RequestIdTruncated,
}

impl From<bit_buf::Error> for ConvertError {
    fn from(e: bit_buf::Error) -> Self {
        ConvertError::BitBuf(e)
    }
}

impl From<nibble_buf::Error> for ConvertError {
    fn from(e: nibble_buf::Error) -> Self {
        ConvertError::NibbleBuf(e)
    }
}

impl From<XpiError> for ConvertError {
    fn from(e: XpiError) -> Self {
        ConvertError::XpiError(e)
    }
}

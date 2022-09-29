use vhl_stdlib_nostd::serdes::{bit_buf, nibble_buf};

pub enum ConvertError {
    BitBuf(bit_buf::Error),
    NibbleBuf(nibble_buf::Error),
    NodeIdTruncate,
    PriorityTruncate,
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

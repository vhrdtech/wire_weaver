use vhl_stdlib_nostd::serdes::{bit_buf, nibble_buf};

/// Error used locally (not transferred across the wire).
#[derive(Debug, Eq, PartialEq)]
pub enum XwfdError {
    NibbleBuf(nibble_buf::Error),
    BitBuf(bit_buf::Error),
    // Unreachable reached
    InternalError,
    UriMaskReserved,
    UriMaskUnsupportedType,

    // Unsupported reserved value, not ignorable.
    ReservedDiscard,

    Unimplemented,
    NodeIdAbove127,

}

impl From<nibble_buf::Error> for XwfdError {
    fn from(e: nibble_buf::Error) -> Self {
        XwfdError::NibbleBuf(e)
    }
}

impl From<bit_buf::Error> for XwfdError {
    fn from(e: bit_buf::Error) -> Self {
        XwfdError::BitBuf(e)
    }
}

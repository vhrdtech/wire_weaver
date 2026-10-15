//! API of the borrowed stream error tests (API-35): stream functions whose error type borrows, the handler returns
//! it borrowed from the server (`RpcResult<Result<(), OpenError<'_>>>`).
use wire_weaver::prelude::*;

/// Plain and counted stream functions, for both the sync and the async server
#[ww_trait]
trait BorrowedError {
    /// Refused with `OpenError::Busy` while the device's owner name isn't empty
    #[stream(u8)]
    fn plain(fail: bool) -> Result<(), OpenError<'i>>;

    #[stream(u8, counted)]
    fn counted(fail: bool) -> Result<(), OpenError<'i>>;
}

/// Adds a reliable stream function, async server only
#[ww_trait]
trait BorrowedErrorReliable {
    #[stream(u8)]
    fn plain(fail: bool) -> Result<(), OpenError<'i>>;

    #[stream(u8, counted)]
    fn counted(fail: bool) -> Result<(), OpenError<'i>>;

    #[stream(u8, reliable)]
    fn reliable(fail: bool) -> Result<(), OpenError<'i>>;
}

/// Error with a borrowed field and a borrowed error nested inside
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone), ww_repr = u4)]
pub enum OpenError<'i> {
    Busy { by: &'i str },
    Inner(InnerError<'i>),
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone), ww_repr = u4)]
pub enum InnerError<'i> {
    Message(&'i str),
}

//! Shared paths of the generated server dispatcher (SRV-51, SRV-52): what every arm of every level would otherwise
//! carry inline lives here once, `#[inline(never)]`, so a firmware pays for it once instead of per resource.
//!
//! The generated code calls these; they are public for it, not meant to be called by hand. Each takes the
//! handler's whole result and does what every arm did with it: start the reply's event kind, serialize the value,
//! write a user error, or answer `Unimplemented`. They are generic over the value and user error types, so a
//! firmware has one copy per type, not one per resource.
//!
//! Errors are written where they happen (SRV-52): [err] rewinds the writer to where the reply starts
//! ([Dispatch::before]) and writes the [Error] there, and the level returns [WrAction::WrittenErr]. Nothing but a
//! one or two byte [WrAction] goes up through the levels, so no `Result` is copied from level to level.

use crate::builder::ErrorBuilder;
use crate::stream::StreamOpened;
use crate::{Error, ErrorKind, ErrorKindDiscriminants, Request, StreamSideband, err_seq};
use wire_weaver::shrink_wrap::{
    BufReader, BufWriter, DeserializeShrinkWrap, Error as ShrinkWrapError, RefVec,
    SerializeShrinkWrap, buf_writer::BufWriterState, nib32::UNib32,
};
use wire_weaver::{GetResult, RpcResult, SetResult};

/// What a level of the generated dispatcher did with a request. The reply, value or error, is in the writer the
/// level got; the root fills in the event kind and the result flag.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WrAction {
    /// An `Ok` reply was written: its event kind's nibble at [Dispatch::before] (not in a multi request's
    /// element), then the value if it has one.
    WrittenOk,
    /// An error was written at [Dispatch::before].
    WrittenErr,
    /// Request handled, nothing to send back but an acknowledgement: answered with `Written`.
    Acknowledged,
    /// Nothing to send now: the handler answers later, or the reply was sent as a whole event.
    Deferred,
    /// Not even the error fits into the writer: the server returns this error and sends nothing.
    Failed(ShrinkWrapError),
}

/// What a level needs to know about the request it handles besides the path ahead: passed down by reference.
/// `MULTI` is the server's `multi_req`: without it no code for elements of a multi request is linked.
#[derive(Clone, Copy)]
pub struct Dispatch<'d, const MULTI: bool> {
    /// The request, or in a multi request the element's (`Read`, `Call`, `Write`).
    pub request: &'d Request<'d>,
    /// The whole path of the resource.
    pub path: &'d [UNib32],
    /// Where the reply starts in the writer: its event kind, or a multi request's element.
    pub before: BufWriterState,
    /// The request is an element of a multi request: no event kind of its own, values written as elements.
    pub in_multi: bool,
}

impl<const MULTI: bool> Dispatch<'_, MULTI> {
    #[inline(always)]
    fn in_multi(&self) -> bool {
        MULTI && self.in_multi
    }
}

/// The error `(err_seq, kind)` as the reply, written where the reply starts: the one cold path of every error site
/// of the dispatcher, so each site is two constants and a call.
#[inline(always)]
pub fn err<const MULTI: bool>(
    wr: &mut BufWriter<'_>,
    d: &Dispatch<'_, MULTI>,
    err_seq: u32,
    kind: ErrorKindDiscriminants,
) -> WrAction {
    write_err(wr, &d.before, err_seq, kind)
}

/// [err] for a site whose [ErrorKind] is only known at run time.
#[inline(always)]
pub fn err_kind<const MULTI: bool>(
    wr: &mut BufWriter<'_>,
    d: &Dispatch<'_, MULTI>,
    err_seq: u32,
    kind: ErrorKind<'_>,
) -> WrAction {
    write_error_at(wr, &d.before, &Error::new(err_seq, kind))
}

#[cold]
#[inline(never)]
fn write_err(
    wr: &mut BufWriter<'_>,
    before: &BufWriterState,
    err_seq: u32,
    kind: ErrorKindDiscriminants,
) -> WrAction {
    use ErrorKindDiscriminants as D;
    let kind = match kind {
        D::OperationNotSupported => ErrorKind::OperationNotSupported,
        D::BadPath => ErrorKind::BadPath,
        D::BadIndex => ErrorKind::BadIndex,
        D::ExpectedArrayIndexGotNone => ErrorKind::ExpectedArrayIndexGotNone,
        D::ArrayIndexDesFailed => ErrorKind::ArrayIndexDesFailed,
        D::ArgsDesFailed => ErrorKind::ArgsDesFailed,
        D::PathDesFailed => ErrorKind::PathDesFailed,
        D::PropertyDesFailed => ErrorKind::PropertyDesFailed,
        D::ResponseSerFailed => ErrorKind::ResponseSerFailed,
        D::Unimplemented => ErrorKind::Unimplemented,
        D::ReadPropertyWithSeqZero => ErrorKind::ReadPropertyWithSeqZero,
        D::PathKindNotSupported => ErrorKind::PathKindNotSupported,
        D::UserBytes => ErrorKind::UserBytes(RefVec::new_bytes(&[])),
        D::UserStr => ErrorKind::UserStr(""),
        D::TraitNotFound => ErrorKind::TraitNotFound,
        D::StreamNoSession => ErrorKind::StreamNoSession,
        D::StreamStoreFailed => ErrorKind::StreamStoreFailed,
        // not a kind of this version: the dispatcher never raises one
        D::Unknown(_) => ErrorKind::OperationNotSupported,
    };
    write_error_at(wr, before, &Error::new(err_seq, kind))
}

/// `error` written at `before`, the start of the reply.
#[inline(never)]
pub fn write_error_at(
    wr: &mut BufWriter<'_>,
    before: &BufWriterState,
    error: &Error<'_>,
) -> WrAction {
    // a handler or a reply path could have started to write before it failed
    wr.restore_state(*before);
    match wr.write(error) {
        Ok(()) => WrAction::WrittenErr,
        Err(e) => WrAction::Failed(e),
    }
}

/// Starts an `Ok` answer: its event kind's nibble, filled in at the root (in a multi request's element, nothing).
#[inline(never)]
pub fn start<const MULTI: bool>(
    wr: &mut BufWriter<'_>,
    d: &Dispatch<'_, MULTI>,
) -> Result<(), ShrinkWrapError> {
    if d.in_multi() {
        Ok(())
    } else {
        wr.write_nib_masked(0)
    }
}

/// Writes an answer's value: as the root of what follows, or as an element of a multi request.
#[inline(always)]
fn ser_value<const MULTI: bool, T: SerializeShrinkWrap>(
    wr: &mut BufWriter<'_>,
    d: &Dispatch<'_, MULTI>,
    value: &T,
) -> Result<(), ShrinkWrapError> {
    if d.in_multi() {
        wr.write(value)
    } else {
        value.ser_shrink_wrap(wr)
    }
}

/// An `Ok` answer with `value`: the event kind started, the value written (`err_seq` names the site when it doesn't
/// fit). A read of a property or a constant, the valid indices of an array.
#[inline(never)]
pub fn reply_value<const MULTI: bool, T: SerializeShrinkWrap>(
    wr: &mut BufWriter<'_>,
    d: &Dispatch<'_, MULTI>,
    value: &T,
    err_seq: u32,
) -> WrAction {
    if start(wr, d).is_err() {
        return err(
            wr,
            d,
            err_seq::REPLY_START,
            ErrorKindDiscriminants::ResponseSerFailed,
        );
    }
    if ser_value(wr, d, value).is_err() {
        return err(wr, d, err_seq, ErrorKindDiscriminants::ResponseSerFailed);
    }
    WrAction::WrittenOk
}

/// A call's answer from its handler's result: the output (not written when the request's seq is 0, no answer is
/// sent), nothing for a deferred call (it answers later), or `Unimplemented`.
#[inline(never)]
pub fn reply_call<const MULTI: bool, T: SerializeShrinkWrap>(
    wr: &mut BufWriter<'_>,
    d: &Dispatch<'_, MULTI>,
    result: RpcResult<T>,
) -> WrAction {
    match result {
        RpcResult::Ready(output) => {
            if start(wr, d).is_err() {
                return err(
                    wr,
                    d,
                    err_seq::REPLY_START,
                    ErrorKindDiscriminants::ResponseSerFailed,
                );
            }
            if d.request.seq.0 != 0 && ser_value(wr, d, &output).is_err() {
                return err(
                    wr,
                    d,
                    err_seq::method::OUTPUT,
                    ErrorKindDiscriminants::ResponseSerFailed,
                );
            }
            WrAction::WrittenOk
        }
        RpcResult::Deferred => WrAction::Deferred,
        RpcResult::Unimplemented => err(
            wr,
            d,
            err_seq::method::HANDLER,
            ErrorKindDiscriminants::Unimplemented,
        ),
    }
}

/// A property read's answer from its getter's result: the value, the user error `E` written in place, or
/// `Unimplemented` written in place.
#[inline(never)]
pub fn reply_get<const MULTI: bool, T: SerializeShrinkWrap, E: SerializeShrinkWrap>(
    wr: &mut BufWriter<'_>,
    d: &Dispatch<'_, MULTI>,
    result: GetResult<T, E>,
) -> WrAction {
    match result {
        GetResult::Value(value) => reply_value(wr, d, &value, err_seq::property::VALUE),
        GetResult::GetError(user_err) => write_user_error(wr, d, &user_err),
        GetResult::Unimplemented => unimplemented_property(wr, d),
    }
}

/// [reply_get] of a property without a user error type: its errors are an empty [ErrorKind::UserBytes].
#[inline(never)]
pub fn reply_get_unit_err<const MULTI: bool, T: SerializeShrinkWrap>(
    wr: &mut BufWriter<'_>,
    d: &Dispatch<'_, MULTI>,
    result: GetResult<T, ()>,
) -> WrAction {
    match result {
        GetResult::Value(value) => reply_value(wr, d, &value, err_seq::property::VALUE),
        GetResult::GetError(()) => write_unit_user_error(wr, d),
        GetResult::Unimplemented => unimplemented_property(wr, d),
    }
}

/// A property write's answer from its setter's result: an `Ok` with nothing, the user error `E` written in place,
/// or `Unimplemented`. User errors are always sent back, even when seq is 0: they won't reach a call site, but
/// they show up in logs.
#[inline(never)]
pub fn reply_set<const MULTI: bool, E: SerializeShrinkWrap>(
    wr: &mut BufWriter<'_>,
    d: &Dispatch<'_, MULTI>,
    result: SetResult<E>,
) -> WrAction {
    match result {
        SetResult::Set => reply_ok(wr, d),
        SetResult::SetError(user_err) => write_user_error(wr, d, &user_err),
        SetResult::Unimplemented => unimplemented_property(wr, d),
    }
}

/// [reply_set] of a property without a user error type: its errors are an empty [ErrorKind::UserBytes].
#[inline(never)]
pub fn reply_set_unit_err<const MULTI: bool>(
    wr: &mut BufWriter<'_>,
    d: &Dispatch<'_, MULTI>,
    result: SetResult<()>,
) -> WrAction {
    match result {
        SetResult::Set => reply_ok(wr, d),
        SetResult::SetError(()) => write_unit_user_error(wr, d),
        SetResult::Unimplemented => unimplemented_property(wr, d),
    }
}

/// An `Ok` answer without a value: a property or a sink written.
#[inline(never)]
pub fn reply_ok<const MULTI: bool>(wr: &mut BufWriter<'_>, d: &Dispatch<'_, MULTI>) -> WrAction {
    match start(wr, d) {
        Ok(()) => WrAction::WrittenOk,
        Err(_) => err(
            wr,
            d,
            err_seq::REPLY_START,
            ErrorKindDiscriminants::ResponseSerFailed,
        ),
    }
}

/// A getter or setter answered `Unimplemented`.
fn unimplemented_property<const MULTI: bool>(
    wr: &mut BufWriter<'_>,
    d: &Dispatch<'_, MULTI>,
) -> WrAction {
    err(
        wr,
        d,
        err_seq::property::HANDLER,
        ErrorKindDiscriminants::Unimplemented,
    )
}

/// A user error of type `E` written in place, inside [ErrorKind::UserBytes].
fn write_user_error<const MULTI: bool, E: SerializeShrinkWrap>(
    wr: &mut BufWriter<'_>,
    d: &Dispatch<'_, MULTI>,
    user_err: &E,
) -> WrAction {
    let es = err_seq::USER_ERROR;
    let failed = |wr: &mut BufWriter<'_>| err(wr, d, es, ErrorKindDiscriminants::ResponseSerFailed);
    let Ok(builder) = ErrorBuilder::new(es, wr) else {
        return failed(wr);
    };
    if wr.write(user_err).is_err() {
        return failed(wr);
    }
    match builder.finish_with_kind(ErrorKindDiscriminants::UserBytes, wr) {
        Ok(()) => WrAction::WrittenErr,
        Err(_) => failed(wr),
    }
}

/// The user error of a property without a user error type, written in place: an empty [ErrorKind::UserBytes].
fn write_unit_user_error<const MULTI: bool>(
    wr: &mut BufWriter<'_>,
    d: &Dispatch<'_, MULTI>,
) -> WrAction {
    // a user error is not a failure of the reply: the reply can't have started yet, the error is written where
    // the writer is
    match wr.write(&Error::new(
        err_seq::USER_ERROR,
        ErrorKind::UserBytes(RefVec::new_bytes(&[])),
    )) {
        Ok(()) => WrAction::WrittenErr,
        Err(_) => err(
            wr,
            d,
            err_seq::USER_ERROR,
            ErrorKindDiscriminants::ResponseSerFailed,
        ),
    }
}

/// A stream open's answer: the session in `opened` (its head) and the stream function's `output`.
#[inline(never)]
pub fn reply_opened<const MULTI: bool, T: SerializeShrinkWrap>(
    wr: &mut BufWriter<'_>,
    d: &Dispatch<'_, MULTI>,
    opened: &StreamOpened<'_>,
    output: &T,
) -> WrAction {
    if start(wr, d).is_err() {
        return err(
            wr,
            d,
            err_seq::REPLY_START,
            ErrorKindDiscriminants::ResponseSerFailed,
        );
    }
    if opened.ser_head(wr).is_err() || output.ser_shrink_wrap(wr).is_err() {
        return err(
            wr,
            d,
            err_seq::stream::OPENED,
            ErrorKindDiscriminants::ResponseSerFailed,
        );
    }
    WrAction::WrittenOk
}

/// The open answer of a plain stream (no session, nothing kept) from its stream function's result: the closed
/// head and the output; a stream function can't defer, a deferred one is refused.
#[inline(never)]
pub fn reply_open_plain<const MULTI: bool, T: SerializeShrinkWrap>(
    wr: &mut BufWriter<'_>,
    d: &Dispatch<'_, MULTI>,
    result: RpcResult<T>,
) -> WrAction {
    match result {
        RpcResult::Ready(output) => reply_opened(wr, d, &StreamOpened::closed(), &output),
        RpcResult::Deferred => err(
            wr,
            d,
            err_seq::stream::OPEN,
            ErrorKindDiscriminants::OperationNotSupported,
        ),
        RpcResult::Unimplemented => err(
            wr,
            d,
            err_seq::stream::OPEN,
            ErrorKindDiscriminants::Unimplemented,
        ),
    }
}

/// A plain sink's or stream's answer to a sideband: the sideband handler's answer with the resource's path, or
/// only an acknowledgement.
#[inline(never)]
pub fn reply_sideband<const MULTI: bool>(
    wr: &mut BufWriter<'_>,
    d: &Dispatch<'_, MULTI>,
    answer: Option<StreamSideband>,
) -> WrAction {
    let Some(sideband) = answer else {
        // nothing to send back, but the host may wait with the request's seq
        return WrAction::Acknowledged;
    };
    if start(wr, d).is_err() {
        return err(
            wr,
            d,
            err_seq::REPLY_START,
            ErrorKindDiscriminants::ResponseSerFailed,
        );
    }
    if wr.write(&RefVec::Slice { slice: d.path }).is_err() || wr.write(&sideband).is_err() {
        return err(
            wr,
            d,
            err_seq::stream::SIDEBAND,
            ErrorKindDiscriminants::ResponseSerFailed,
        );
    }
    WrAction::WrittenOk
}

/// A value decoded from the bytes of a request (a property write's value, a call's arguments); `None` when they
/// don't decode.
#[inline(never)]
pub fn des<'i, T: DeserializeShrinkWrap<'i>>(bytes: &'i [u8]) -> Option<T> {
    let mut rd = BufReader::new(bytes);
    T::des_shrink_wrap(&mut rd).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PathKind, Priority, RequestKind};
    use wire_weaver::shrink_wrap::UVlq32Backfill;

    #[test]
    fn err_rewinds_and_writes_the_error() {
        let request = Request {
            seq: UVlq32Backfill(1),
            priority: Priority::NORMAL.nibble(),
            path_kind: PathKind::absolute(&[]),
            kind: RequestKind::Read,
        };
        let mut buf = [0u8; 32];
        let mut wr = BufWriter::new(&mut buf);
        wr.write_u8(0xAA).unwrap();
        let d: Dispatch<'_, false> = Dispatch {
            request: &request,
            path: &[],
            before: wr.save_state(),
            in_multi: false,
        };
        wr.write_u8(0xBB).unwrap();
        assert_eq!(
            err(&mut wr, &d, 42, ErrorKindDiscriminants::BadPath),
            WrAction::WrittenErr
        );
        let bytes = wr.finish_and_take().unwrap();
        let mut expected = [0u8; 32];
        let mut ex = BufWriter::new(&mut expected);
        ex.write_u8(0xAA).unwrap();
        ex.write(&Error::new(42, ErrorKind::BadPath)).unwrap();
        assert_eq!(bytes, ex.finish_and_take().unwrap());
    }
}

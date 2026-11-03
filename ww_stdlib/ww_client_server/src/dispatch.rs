//! Shared paths of the generated server dispatcher (SRV-51): what every arm of every level would otherwise carry
//! inline lives here once, `#[inline(never)]`, so a firmware pays for it once instead of per resource.
//!
//! The generated code calls these; they are public for it, not meant to be called by hand. Each takes the
//! handler's whole result and does what every arm did with it: start the reply's event kind, serialize the value,
//! write a user error, or answer `Unimplemented`. They are generic over the value and user error types, so a
//! firmware has one copy per type, not one per resource; the errors go through one cold constructor, [err].

use crate::builder::{ErrorBuilder, EventKindBuilder};
use crate::stream::StreamOpened;
use crate::{Error, ErrorKind, ErrorKindDiscriminants, Request, StreamSideband, err_seq};
use wire_weaver::shrink_wrap::{
    BufReader, BufWriter, DeserializeShrinkWrap, Error as ShrinkWrapError, RefVec,
    SerializeShrinkWrap, nib32::UNib32,
};
use wire_weaver::{GetResult, RpcResult, SetResult};

/// What a level of the generated dispatcher did with a request: the reply is written into the writer it got (or,
/// for [WrAction::Deferred], comes later or went out as a whole event). `K` is the reply's event kind builder: an
/// [EventKindBuilder] in a plain server, `Option<EventKindBuilder>` in a `multi_req` one (`None` in a multi
/// request, whose elements have no event kind of their own).
pub enum WrAction<K = EventKindBuilder> {
    /// A value or nothing was written for an `Ok` reply, the kind is filled in with `K` at the root.
    WrittenOk(K),
    /// An error was written in place (a user error, a read of an unimplemented property).
    WrittenErr,
    /// Request handled, nothing to send back but an acknowledgement: answered with `Written`.
    Acknowledged,
    /// Nothing to send now: the handler answers later, or the reply was sent as a whole event.
    Deferred,
}

/// What a level of the generated dispatcher returns: the [WrAction], or an error the root writes as the reply.
pub type DispatchResult<'a, K = EventKindBuilder> = Result<WrAction<K>, Error<'a>>;

/// The error `(err_seq, kind)` as a level's result: one cold constructor for every error site of the dispatcher,
/// so each site is two constants and a call instead of building the [Error] inline.
#[cold]
#[inline(never)]
pub fn err<'a, K>(err_seq: u32, kind: ErrorKind<'a>) -> DispatchResult<'a, K> {
    Err(Error::new(err_seq, kind))
}

/// The [Error] `(err_seq, kind)`, see [err].
#[cold]
#[inline(never)]
pub fn error(err_seq: u32, kind: ErrorKind<'_>) -> Error<'_> {
    Error::new(err_seq, kind)
}

/// The event kind builder of a reply, `K` of [WrAction]: how a value answer starts and how its value is written.
pub trait ReplyKind: Sized {
    /// Starts an `Ok` answer's event kind (in a multi request, nothing).
    fn start(wr: &mut BufWriter<'_>, in_multi: bool) -> Result<Self, ShrinkWrapError>;

    /// Writes an answer's value: as the root of what follows, or as an element of a multi request.
    fn ser_value<T: SerializeShrinkWrap>(
        wr: &mut BufWriter<'_>,
        in_multi: bool,
        value: &T,
    ) -> Result<(), ShrinkWrapError>;
}

impl ReplyKind for EventKindBuilder {
    // one copy for every reply path, at opt-level 3 too
    #[inline(never)]
    fn start(wr: &mut BufWriter<'_>, _in_multi: bool) -> Result<Self, ShrinkWrapError> {
        EventKindBuilder::new(wr)
    }

    #[inline(always)]
    fn ser_value<T: SerializeShrinkWrap>(
        wr: &mut BufWriter<'_>,
        _in_multi: bool,
        value: &T,
    ) -> Result<(), ShrinkWrapError> {
        value.ser_shrink_wrap(wr)
    }
}

impl ReplyKind for Option<EventKindBuilder> {
    #[inline(never)]
    fn start(wr: &mut BufWriter<'_>, in_multi: bool) -> Result<Self, ShrinkWrapError> {
        if in_multi {
            Ok(None)
        } else {
            EventKindBuilder::new(wr).map(Some)
        }
    }

    #[inline(always)]
    fn ser_value<T: SerializeShrinkWrap>(
        wr: &mut BufWriter<'_>,
        in_multi: bool,
        value: &T,
    ) -> Result<(), ShrinkWrapError> {
        if in_multi {
            wr.write(value)
        } else {
            value.ser_shrink_wrap(wr)
        }
    }
}

/// An `Ok` answer with `value`: the event kind started, the value written (`err_seq` names the site when it doesn't
/// fit). A read of a property or a constant, the valid indices of an array.
#[inline(never)]
pub fn reply_value<'a, K: ReplyKind, T: SerializeShrinkWrap>(
    wr: &mut BufWriter<'_>,
    in_multi: bool,
    value: &T,
    err_seq: u32,
) -> DispatchResult<'a, K> {
    let Ok(kind) = K::start(wr, in_multi) else {
        return err(err_seq::REPLY_START, ErrorKind::ResponseSerFailed);
    };
    if K::ser_value(wr, in_multi, value).is_err() {
        return err(err_seq, ErrorKind::ResponseSerFailed);
    }
    Ok(WrAction::WrittenOk(kind))
}

/// A call's answer from its handler's result: the output (not written when the request's seq is 0, no answer is
/// sent), nothing for a deferred call (it answers later), or `Unimplemented`.
#[inline(never)]
pub fn reply_call<'a, K: ReplyKind, T: SerializeShrinkWrap>(
    wr: &mut BufWriter<'_>,
    request: &Request<'_>,
    in_multi: bool,
    result: RpcResult<T>,
) -> DispatchResult<'a, K> {
    match result {
        RpcResult::Ready(output) => {
            let Ok(kind) = K::start(wr, in_multi) else {
                return err(err_seq::REPLY_START, ErrorKind::ResponseSerFailed);
            };
            if request.seq.0 != 0 && K::ser_value(wr, in_multi, &output).is_err() {
                return err(err_seq::method::OUTPUT, ErrorKind::ResponseSerFailed);
            }
            Ok(WrAction::WrittenOk(kind))
        }
        RpcResult::Deferred => Ok(WrAction::Deferred),
        RpcResult::Unimplemented => err(err_seq::method::HANDLER, ErrorKind::Unimplemented),
    }
}

/// A property read's answer from its getter's result: the value, the user error `E` written in place, or
/// `Unimplemented` written in place.
#[inline(never)]
pub fn reply_get<'a, K: ReplyKind, T: SerializeShrinkWrap, E: SerializeShrinkWrap>(
    wr: &mut BufWriter<'_>,
    in_multi: bool,
    result: GetResult<T, E>,
) -> DispatchResult<'a, K> {
    match result {
        GetResult::Value(value) => reply_value(wr, in_multi, &value, err_seq::property::VALUE),
        GetResult::GetError(user_err) => write_user_error(wr, &user_err),
        GetResult::Unimplemented => {
            write_error(wr, err_seq::property::HANDLER, ErrorKind::Unimplemented)
        }
    }
}

/// [reply_get] of a property without a user error type: its errors are an empty [ErrorKind::UserBytes].
#[inline(never)]
pub fn reply_get_unit_err<'a, K: ReplyKind, T: SerializeShrinkWrap>(
    wr: &mut BufWriter<'_>,
    in_multi: bool,
    result: GetResult<T, ()>,
) -> DispatchResult<'a, K> {
    match result {
        GetResult::Value(value) => reply_value(wr, in_multi, &value, err_seq::property::VALUE),
        GetResult::GetError(()) => write_unit_user_error(wr),
        GetResult::Unimplemented => {
            write_error(wr, err_seq::property::HANDLER, ErrorKind::Unimplemented)
        }
    }
}

/// A property write's answer from its setter's result: an `Ok` with nothing, the user error `E` written in place,
/// or `Unimplemented`. User errors are always sent back, even when seq is 0: they won't reach a call site, but
/// they show up in logs.
#[inline(never)]
pub fn reply_set<'a, K: ReplyKind, E: SerializeShrinkWrap>(
    wr: &mut BufWriter<'_>,
    in_multi: bool,
    result: SetResult<E>,
) -> DispatchResult<'a, K> {
    match result {
        SetResult::Set => reply_ok(wr, in_multi),
        SetResult::SetError(user_err) => write_user_error(wr, &user_err),
        SetResult::Unimplemented => err(err_seq::property::HANDLER, ErrorKind::Unimplemented),
    }
}

/// [reply_set] of a property without a user error type: its errors are an empty [ErrorKind::UserBytes].
#[inline(never)]
pub fn reply_set_unit_err<'a, K: ReplyKind>(
    wr: &mut BufWriter<'_>,
    in_multi: bool,
    result: SetResult<()>,
) -> DispatchResult<'a, K> {
    match result {
        SetResult::Set => reply_ok(wr, in_multi),
        SetResult::SetError(()) => write_unit_user_error(wr),
        SetResult::Unimplemented => err(err_seq::property::HANDLER, ErrorKind::Unimplemented),
    }
}

/// An `Ok` answer without a value: a property written.
#[inline(never)]
pub fn reply_ok<'a, K: ReplyKind>(wr: &mut BufWriter<'_>, in_multi: bool) -> DispatchResult<'a, K> {
    match K::start(wr, in_multi) {
        Ok(kind) => Ok(WrAction::WrittenOk(kind)),
        Err(_) => err(err_seq::REPLY_START, ErrorKind::ResponseSerFailed),
    }
}

/// A user error of type `E` written in place, inside [ErrorKind::UserBytes].
fn write_user_error<'a, K, E: SerializeShrinkWrap>(
    wr: &mut BufWriter<'_>,
    user_err: &E,
) -> DispatchResult<'a, K> {
    let es = err_seq::USER_ERROR;
    let Ok(builder) = ErrorBuilder::new(es, wr) else {
        return err(es, ErrorKind::ResponseSerFailed);
    };
    if wr.write(user_err).is_err() {
        return err(es, ErrorKind::ResponseSerFailed);
    }
    match builder.finish_with_kind(ErrorKindDiscriminants::UserBytes, wr) {
        Ok(()) => Ok(WrAction::WrittenErr),
        Err(_) => err(es, ErrorKind::ResponseSerFailed),
    }
}

/// The user error of a property without a user error type, written in place: an empty [ErrorKind::UserBytes].
fn write_unit_user_error<'a, K>(wr: &mut BufWriter<'_>) -> DispatchResult<'a, K> {
    write_error(
        wr,
        err_seq::USER_ERROR,
        ErrorKind::UserBytes(RefVec::new_bytes(&[])),
    )
}

/// A stream open's answer: the session in `opened` (its head) and the stream function's `output`.
#[inline(never)]
pub fn reply_opened<'a, K: ReplyKind, T: SerializeShrinkWrap>(
    wr: &mut BufWriter<'_>,
    in_multi: bool,
    opened: &StreamOpened<'_>,
    output: &T,
) -> DispatchResult<'a, K> {
    let Ok(kind) = K::start(wr, in_multi) else {
        return err(err_seq::REPLY_START, ErrorKind::ResponseSerFailed);
    };
    if opened.ser_head(wr).is_err() || output.ser_shrink_wrap(wr).is_err() {
        return err(err_seq::stream::OPENED, ErrorKind::ResponseSerFailed);
    }
    Ok(WrAction::WrittenOk(kind))
}

/// The open answer of a plain stream (no session, nothing kept) from its stream function's result: the closed
/// head and the output; a stream function can't defer, a deferred one is refused.
#[inline(never)]
pub fn reply_open_plain<'a, K: ReplyKind, T: SerializeShrinkWrap>(
    wr: &mut BufWriter<'_>,
    in_multi: bool,
    result: RpcResult<T>,
) -> DispatchResult<'a, K> {
    match result {
        RpcResult::Ready(output) => reply_opened(wr, in_multi, &StreamOpened::closed(), &output),
        RpcResult::Deferred => err(err_seq::stream::OPEN, ErrorKind::OperationNotSupported),
        RpcResult::Unimplemented => err(err_seq::stream::OPEN, ErrorKind::Unimplemented),
    }
}

/// A plain sink's or stream's answer to a sideband: the sideband handler's answer with the resource's `path`, or
/// only an acknowledgement.
#[inline(never)]
pub fn reply_sideband<'a, K: ReplyKind>(
    wr: &mut BufWriter<'_>,
    in_multi: bool,
    path: &[UNib32],
    answer: Option<StreamSideband>,
) -> DispatchResult<'a, K> {
    let Some(sideband) = answer else {
        // nothing to send back, but the host may wait with the request's seq
        return Ok(WrAction::Acknowledged);
    };
    let Ok(kind) = K::start(wr, in_multi) else {
        return err(err_seq::REPLY_START, ErrorKind::ResponseSerFailed);
    };
    if wr.write(&RefVec::Slice { slice: path }).is_err() || wr.write(&sideband).is_err() {
        return err(err_seq::stream::SIDEBAND, ErrorKind::ResponseSerFailed);
    }
    Ok(WrAction::WrittenOk(kind))
}

/// The error `(err_seq, kind)` written in place as the answer.
#[inline(never)]
pub fn write_error<'a, K>(
    wr: &mut BufWriter<'_>,
    err_seq: u32,
    kind: ErrorKind<'_>,
) -> DispatchResult<'a, K> {
    match wr.write(&Error::new(err_seq, kind)) {
        Ok(()) => Ok(WrAction::WrittenErr),
        Err(_) => err(err_seq, ErrorKind::ResponseSerFailed),
    }
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

    #[test]
    fn err_is_the_error_of_its_kind() {
        let r: DispatchResult<'_> = err(42, ErrorKind::BadPath);
        let Err(e) = r else { panic!() };
        assert_eq!(e.err_seq(), 42);
        assert!(matches!(e.kind(), ErrorKind::BadPath));
        let e = error(3, ErrorKind::Unimplemented);
        assert!(matches!(e.kind(), ErrorKind::Unimplemented));
    }
}

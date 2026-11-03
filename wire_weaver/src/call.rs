//! What a call through an API's local trait (CLT-57) gives besides its value, without the client runtime (SRV-48):
//! [Error] and [CallError] for the ways a call fails, [ItemStream] for the items of a stream, [BoxFuture] for the
//! trait's `Dyn` companion.
//!
//! The generated client, a local implementation and (later) a trait server all speak these types, so an API crate's
//! `local` feature needs `wire_weaver` with `std` only. `wire_weaver_client`'s errors convert into them and stay
//! reachable as their [source](std::error::Error::source).

use exact_units::Bytes;
use std::convert::Infallible;
use std::fmt::{Debug, Display, Formatter};
use std::future::Future;
use std::pin::Pin;

/// Future of a function of a local trait's `Dyn` companion.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Items of a stream opened through a local trait: from a device (`wire_weaver_client::Stream::into_item_stream`)
/// or from a local implementation ([item_stream]). Ends when the stream is closed; an error is the last item.
pub type ItemStream<T> = Pin<Box<dyn futures_core::Stream<Item = Result<T, Error>> + Send>>;

/// An [ItemStream] of a local implementation, e.g. from `futures_lite::stream::iter` or a channel receiver.
pub fn item_stream<T>(
    stream: impl futures_core::Stream<Item = Result<T, Error>> + Send + 'static,
) -> ItemStream<T> {
    Box::pin(stream)
}

/// The cause a call error carries: a message, another error (the client's, kept whole), or nothing.
pub type Source = Box<dyn std::error::Error + Send + Sync + 'static>;

/// What went wrong with a call, without the API's own error ([CallError::Api]).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// The callee doesn't implement the item: a handler that answered `Unimplemented`, a device whose API has no
    /// such item, an item a local implementation leaves out.
    Unimplemented,
    /// The caller may not call it (SRV-46).
    Denied,
    /// The callee can't be reached or went away: not connected, disconnected, the transport failed. Trying again
    /// later may work.
    Unavailable,
    /// The call's deadline passed, or the client's timeout, before the answer came.
    DeadlineExceeded,
    /// The call was cancelled before it finished.
    Cancelled,
    /// The callee is busy: too many calls in flight. Trying again later may work.
    Overloaded,
    /// A request or reply is longer than the link's message limit (CLT-55): which one, how long, and the limit.
    /// Trying again won't help; a smaller request, or a result in pieces (a stream, pages), does.
    MessageTooBig(MessageTooBig),
    /// Anything else: a broken reply, an incompatible API, a bug.
    Internal,
}

/// Which message of a call was over the link's limit ([ErrorKind::MessageTooBig]).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum MessageKind {
    /// The caller's request: refused before it is sent.
    Request,
    /// The callee's reply: the callee answers with this error instead.
    Reply,
}

/// A request or reply over the link's message limit ([ErrorKind::MessageTooBig]).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub struct MessageTooBig {
    pub message: MessageKind,
    /// The message's length, None where the side that refused it doesn't say (a device refusing its reply).
    pub len: Option<Bytes<u32>>,
    /// The link's limit: the smaller of the two sides'.
    pub limit: Bytes<u32>,
}

impl MessageTooBig {
    pub const fn new(message: MessageKind, len: Option<Bytes<u32>>, limit: Bytes<u32>) -> Self {
        MessageTooBig {
            message,
            len,
            limit,
        }
    }
}

/// `request` or `reply`
impl Display for MessageKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            MessageKind::Request => "request",
            MessageKind::Reply => "reply",
        })
    }
}

impl Display for MessageTooBig {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let what = self.message;
        match self.len {
            Some(len) => write!(
                f,
                "{what} of {len} over the link's message limit of {}",
                self.limit
            ),
            None => write!(f, "{what} over the link's message limit of {}", self.limit),
        }
    }
}

impl ErrorKind {
    fn describe(self) -> &'static str {
        match self {
            ErrorKind::Unimplemented => "not implemented",
            ErrorKind::Denied => "denied",
            ErrorKind::Unavailable => "unavailable",
            ErrorKind::DeadlineExceeded => "deadline exceeded",
            ErrorKind::Cancelled => "cancelled",
            ErrorKind::Overloaded => "overloaded",
            ErrorKind::MessageTooBig(_) => "message too big",
            ErrorKind::Internal => "internal error",
        }
    }
}

impl Display for ErrorKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            ErrorKind::MessageTooBig(m) => Display::fmt(m, f),
            _ => f.write_str(self.describe()),
        }
    }
}

/// A call failed on its way, or the callee couldn't serve it: an [ErrorKind] and what caused it.
///
/// `Display` is the cause's text where there is one (`'gpio[].set_high' is not implemented by the connected
/// device`), else the kind's (`denied`); [source](std::error::Error::source) is the cause's own source.
pub struct Error {
    kind: ErrorKind,
    source: Option<Source>,
}

impl Error {
    pub fn new(kind: ErrorKind, source: impl Into<Source>) -> Self {
        Error {
            kind,
            source: Some(source.into()),
        }
    }

    /// An error of `kind` without a cause.
    pub const fn from_kind(kind: ErrorKind) -> Self {
        Error { kind, source: None }
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The cause, e.g. a `wire_weaver_client::Error` to downcast to.
    pub fn cause(&self) -> Option<&(dyn std::error::Error + Send + Sync + 'static)> {
        self.source.as_deref()
    }

    /// Takes the cause out, e.g. to downcast it by value.
    pub fn into_cause(self) -> Option<Source> {
        self.source
    }

    pub fn unimplemented(what: impl Into<Source>) -> Self {
        Self::new(ErrorKind::Unimplemented, what)
    }

    pub fn denied(why: impl Into<Source>) -> Self {
        Self::new(ErrorKind::Denied, why)
    }

    pub fn unavailable(why: impl Into<Source>) -> Self {
        Self::new(ErrorKind::Unavailable, why)
    }

    pub fn internal(why: impl Into<Source>) -> Self {
        Self::new(ErrorKind::Internal, why)
    }

    /// A request or reply over the link's limit, without a cause: its text says which, how long and the limit.
    pub const fn message_too_big(too_big: MessageTooBig) -> Self {
        Self::from_kind(ErrorKind::MessageTooBig(too_big))
    }

    /// Which message was over the link's limit, how long and the limit, for [ErrorKind::MessageTooBig].
    pub fn too_big(&self) -> Option<MessageTooBig> {
        match self.kind {
            ErrorKind::MessageTooBig(m) => Some(m),
            _ => None,
        }
    }
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Self {
        Error::from_kind(kind)
    }
}

impl Debug for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.source {
            Some(source) => write!(f, "{:?}({source:?})", self.kind),
            None => write!(f, "{:?}", self.kind),
        }
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.source {
            Some(source) => Display::fmt(source, f),
            None => Display::fmt(&self.kind, f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_deref()?.source()
    }
}

impl From<Infallible> for Error {
    fn from(e: Infallible) -> Self {
        match e {}
    }
}

/// A call to a function returning `Result<T, E>` failed: with the API's error ([CallError::Api]), or on its way or
/// unserved (the other variants, one per [ErrorKind], each with its cause if there is one).
///
/// One `?` takes an [Error] into a `Result<_, CallError<E>>`, and a match tells the outcomes apart:
///
/// ```
/// # use wire_weaver::call::{CallError, Error, ErrorKind};
/// # #[derive(Debug)] enum CounterError { DivByZero }
/// fn report(r: Result<u32, CallError<CounterError>>) -> &'static str {
///     match r {
///         Ok(_) => "ok",
///         Err(CallError::Api(CounterError::DivByZero)) => "division by zero",
///         Err(CallError::Unimplemented(_)) => "not there",
///         Err(_) => "failed on its way",
///     }
/// }
/// assert_eq!(report(Err(CallError::Api(CounterError::DivByZero))), "division by zero");
/// assert_eq!(report(Err(Error::from_kind(ErrorKind::Unimplemented).into())), "not there");
/// ```
///
/// `Display` and [source](std::error::Error::source) are transparent: the inner error's text, once.
#[non_exhaustive]
pub enum CallError<E> {
    /// The callee answered with the API's error: the `Err(e)` of the method (or of the property write).
    Api(E),
    /// [ErrorKind::Unimplemented]
    Unimplemented(Option<Source>),
    /// [ErrorKind::Denied]
    Denied(Option<Source>),
    /// [ErrorKind::Unavailable]
    Unavailable(Option<Source>),
    /// [ErrorKind::DeadlineExceeded]
    DeadlineExceeded(Option<Source>),
    /// [ErrorKind::Cancelled]
    Cancelled(Option<Source>),
    /// [ErrorKind::Overloaded]
    Overloaded(Option<Source>),
    /// [ErrorKind::MessageTooBig]: which message, how long and the link's limit.
    MessageTooBig(MessageTooBig, Option<Source>),
    /// [ErrorKind::Internal]
    Internal(Option<Source>),
}

impl<E> CallError<E> {
    /// The API's error, if the callee answered with one.
    pub fn api(&self) -> Option<&E> {
        match self {
            CallError::Api(e) => Some(e),
            _ => None,
        }
    }

    /// What went wrong on the way, None for the API's error.
    pub fn kind(&self) -> Option<ErrorKind> {
        Some(match self {
            CallError::Api(_) => return None,
            CallError::Unimplemented(_) => ErrorKind::Unimplemented,
            CallError::Denied(_) => ErrorKind::Denied,
            CallError::Unavailable(_) => ErrorKind::Unavailable,
            CallError::DeadlineExceeded(_) => ErrorKind::DeadlineExceeded,
            CallError::Cancelled(_) => ErrorKind::Cancelled,
            CallError::Overloaded(_) => ErrorKind::Overloaded,
            CallError::MessageTooBig(m, _) => ErrorKind::MessageTooBig(*m),
            CallError::Internal(_) => ErrorKind::Internal,
        })
    }

    /// The callee doesn't implement the item ([ErrorKind::Unimplemented]).
    pub fn is_unimplemented(&self) -> bool {
        matches!(self, CallError::Unimplemented(_))
    }

    /// The API's error as `Ok`, or the call's error as an [Error].
    pub fn into_api(self) -> Result<E, Error> {
        let Some(kind) = self.kind() else {
            let CallError::Api(e) = self else {
                unreachable!("only Api has no kind")
            };
            return Ok(e);
        };
        let source = match self {
            CallError::Api(e) => return Ok(e),
            CallError::Unimplemented(s)
            | CallError::Denied(s)
            | CallError::Unavailable(s)
            | CallError::DeadlineExceeded(s)
            | CallError::Cancelled(s)
            | CallError::Overloaded(s)
            | CallError::MessageTooBig(_, s)
            | CallError::Internal(s) => s,
        };
        Err(Error { kind, source })
    }

    fn cause(&self) -> Option<&Source> {
        match self {
            CallError::Api(_) => None,
            CallError::Unimplemented(s)
            | CallError::Denied(s)
            | CallError::Unavailable(s)
            | CallError::DeadlineExceeded(s)
            | CallError::Cancelled(s)
            | CallError::Overloaded(s)
            | CallError::MessageTooBig(_, s)
            | CallError::Internal(s) => s.as_ref(),
        }
    }

    /// The API's error through `f`, the others as they are.
    pub fn map_api<F>(self, f: impl FnOnce(E) -> F) -> CallError<F> {
        match self.into_api() {
            Ok(e) => CallError::Api(f(e)),
            Err(e) => e.into(),
        }
    }
}

impl<E> From<Error> for CallError<E> {
    fn from(e: Error) -> Self {
        let s = e.source;
        match e.kind {
            ErrorKind::Unimplemented => CallError::Unimplemented(s),
            ErrorKind::Denied => CallError::Denied(s),
            ErrorKind::Unavailable => CallError::Unavailable(s),
            ErrorKind::DeadlineExceeded => CallError::DeadlineExceeded(s),
            ErrorKind::Cancelled => CallError::Cancelled(s),
            ErrorKind::Overloaded => CallError::Overloaded(s),
            ErrorKind::MessageTooBig(m) => CallError::MessageTooBig(m, s),
            ErrorKind::Internal => CallError::Internal(s),
        }
    }
}

impl<E> From<ErrorKind> for CallError<E> {
    fn from(kind: ErrorKind) -> Self {
        Error::from_kind(kind).into()
    }
}

impl<E: Debug> Debug for CallError<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match (self.kind(), self.cause()) {
            (None, _) => f.debug_tuple("Api").field(self.api().unwrap()).finish(),
            (Some(kind), Some(cause)) => write!(f, "{kind:?}({cause:?})"),
            (Some(kind), None) => write!(f, "{kind:?}"),
        }
    }
}

impl<E: Display> Display for CallError<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match (self, self.cause()) {
            (CallError::Api(e), _) => Display::fmt(e, f),
            (_, Some(cause)) => Display::fmt(cause, f),
            (_, None) => Display::fmt(&self.kind().unwrap(), f),
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for CallError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CallError::Api(e) => e.source(),
            _ => self.cause()?.source(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_is_the_cause_or_the_kind() {
        assert_eq!(Error::from_kind(ErrorKind::Denied).to_string(), "denied");
        assert_eq!(Error::denied("uid 1001").to_string(), "uid 1001");
        let e: CallError<std::fmt::Error> = Error::internal("broken reply").into();
        assert_eq!(e.to_string(), "broken reply");
        assert!(matches!(e, CallError::Internal(Some(_))));
        assert_eq!(e.kind(), Some(ErrorKind::Internal));
        assert!(CallError::<u8>::from(ErrorKind::Unimplemented).is_unimplemented());
        assert_eq!(CallError::<u8>::Denied(None).to_string(), "denied");
        assert_eq!(CallError::<u8>::Api(3).into_api().unwrap(), 3);
        let back = CallError::<u8>::Cancelled(None).into_api().unwrap_err();
        assert_eq!(back.kind(), ErrorKind::Cancelled);
    }

    #[test]
    fn cause_is_kept() {
        #[derive(Debug)]
        struct Inner;
        impl Display for Inner {
            fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
                f.write_str("inner")
            }
        }
        impl std::error::Error for Inner {}

        let e = Error::unavailable(Inner);
        assert!(e.cause().unwrap().downcast_ref::<Inner>().is_some());
        assert!(e.into_cause().unwrap().downcast::<Inner>().is_ok());
    }

    #[test]
    fn message_too_big_carries_its_numbers() {
        let m = MessageTooBig::new(
            MessageKind::Reply,
            Some(Bytes::new(70_000)),
            Bytes::new(65_536),
        );
        let e = Error::message_too_big(m);
        assert_eq!(e.kind(), ErrorKind::MessageTooBig(m));
        assert_eq!(e.too_big(), Some(m));
        assert_eq!(
            e.to_string(),
            "reply of 70000 B over the link's message limit of 65536 B"
        );
        let c: CallError<u8> = e.into();
        assert!(matches!(c, CallError::MessageTooBig(got, None) if got == m));
        assert_eq!(c.into_api().unwrap_err().too_big(), Some(m));
        let request = MessageTooBig::new(MessageKind::Request, None, Bytes::new(512));
        assert_eq!(
            request.to_string(),
            "request over the link's message limit of 512 B"
        );
    }

    #[test]
    fn question_mark_into_call_error() {
        fn inner() -> Result<(), Error> {
            Err(ErrorKind::Overloaded.into())
        }
        fn outer() -> Result<(), CallError<u8>> {
            inner()?;
            Ok(())
        }
        assert_eq!(outer().unwrap_err().kind(), Some(ErrorKind::Overloaded));
    }
}

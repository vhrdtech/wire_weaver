use ww_api::ww_error::CommonError;

/// Result type that each server handler returns.
/// - `Ready(T)` - serialize T and send it back to a client that is making a request.
/// - `Deferred` - do nothing, result must be sent later using `cx.request_id` provided.
/// - `Unimplemented` - send `ww_client_server::Error::Unimplemented` back to a client.
///     - No need to return sentinel values when handler is not yet ready
///     - Safe default for codegen from CLI
/// - `Failed` - a host server's own answer, from a call through the API's local trait ([Failure], feature `std`).
///
/// Note that `T` can itself be `Result<UserOkTy, UserErrTy>`.
///
/// ## Why not use Result<UserTy, WwError>?
/// Confusing Ok(Ok(ok_user_value)), Ok(Err(err_user_value)), Err(ww_err) statements in every handler.
pub enum RpcResult<T> {
    Ready(T),
    Deferred,
    Unimplemented,
    Failed(Failure),
}

impl<O> RpcResult<O> {
    /// The handler's output of an item declared without a `Result` as what the dispatcher writes: the value, or the
    /// common error the handler answered with instead ([IntoOutput]). `T` is the item's declared type, so a handler
    /// that returns another one is refused where it is called.
    #[inline(always)]
    pub fn output<'i, T>(self) -> RpcResult<Result<T, CommonError<'i>>>
    where
        O: IntoOutput<'i, T>,
    {
        match self {
            RpcResult::Ready(output) => RpcResult::Ready(output.into_output()),
            RpcResult::Deferred => RpcResult::Deferred,
            RpcResult::Unimplemented => RpcResult::Unimplemented,
            RpcResult::Failed(f) => {
                f.inhabited();
                RpcResult::Failed(f)
            }
        }
    }
}

pub enum SetResult<E> {
    Set,
    SetError(E),
    Unimplemented,
    /// A host server's own answer ([Failure]).
    Failed(Failure),
}

pub enum GetResult<T, E> {
    Value(T),
    GetError(E),
    Unimplemented,
    /// A host server's own answer ([Failure]).
    Failed(Failure),
}

/// A handler's answer other than its value, the API's error and a common error it can borrow: what a call through
/// the API's local trait failed with (SRV-49). The generated server writes it as `Denied`, `DeadlineExceeded`,
/// `Cancelled`, `Overloaded`, `Internal`, `Unimplemented` or the common error the call answered with.
///
/// Feature `std`: without it the type has no value, so a firmware links no code for the `Failed` variants.
pub struct Failure(FailureInner);

#[cfg(feature = "std")]
type FailureInner = crate::call::Error;
#[cfg(not(feature = "std"))]
type FailureInner = core::convert::Infallible;

impl Failure {
    /// Nothing with `std`. Without it there is no failure, so code after this call is unreachable and dropped: the
    /// dispatcher calls it first in every `Failed` arm, and a firmware links none of them.
    #[inline(always)]
    pub fn inhabited(&self) {
        #[cfg(not(feature = "std"))]
        match self.0 {}
    }
}

#[cfg(feature = "std")]
impl Failure {
    /// The call's error.
    pub fn error(&self) -> &crate::call::Error {
        &self.0
    }

    /// The common error the call answered with, borrowed for writing it.
    pub fn common(&self) -> Option<CommonError<'_>> {
        use shrink_wrap::MaxLen;
        use ww_api::ww_error::CommonErrorOwned as O;
        fn s<const N: usize>(s: &MaxLen<std::string::String, N>) -> MaxLen<&str, N> {
            MaxLen::new_unchecked(s.as_str())
        }
        Some(match self.0.common_error()? {
            O::Unsupported => CommonError::Unsupported,
            O::InvalidArgument { arg, reason } => CommonError::InvalidArgument {
                arg: *arg,
                reason: s(reason),
            },
            O::InUse { by } => CommonError::InUse { by: s(by) },
            O::Busy => CommonError::Busy,
            O::WrongState { reason } => CommonError::WrongState { reason: s(reason) },
            O::NoPower { rail } => CommonError::NoPower { rail: s(rail) },
            O::Refused { reason } => CommonError::Refused { reason: s(reason) },
            O::Timeout => CommonError::Timeout,
            O::Fault { code } => CommonError::Fault { code: *code },
            O::Unknown(n) => CommonError::Unknown(*n),
        })
    }
}

#[cfg(feature = "std")]
impl From<crate::call::Error> for Failure {
    fn from(e: crate::call::Error) -> Self {
        Failure(e)
    }
}

#[cfg(feature = "std")]
impl core::fmt::Debug for Failure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(&self.0, f)
    }
}

#[cfg(not(feature = "std"))]
impl core::fmt::Debug for Failure {
    fn fmt(&self, _f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {}
    }
}

impl<T> From<Failure> for RpcResult<T> {
    fn from(f: Failure) -> Self {
        RpcResult::Failed(f)
    }
}

impl<E> From<Failure> for SetResult<E> {
    fn from(f: Failure) -> Self {
        SetResult::Failed(f)
    }
}

impl<T, E> From<Failure> for GetResult<T, E> {
    fn from(f: Failure) -> Self {
        GetResult::Failed(f)
    }
}

pub struct Unimplemented;

impl<T> From<Unimplemented> for RpcResult<T> {
    fn from(_: Unimplemented) -> Self {
        RpcResult::Unimplemented
    }
}

impl<E> From<Unimplemented> for SetResult<E> {
    fn from(_: Unimplemented) -> Self {
        SetResult::Unimplemented
    }
}

impl<T, E> From<Unimplemented> for GetResult<T, E> {
    fn from(_: Unimplemented) -> Self {
        GetResult::Unimplemented
    }
}

/// Shorthand for `wire_weaver::Unimplemented.into()`.
/// Can be used in functions returning `RpcResult`, `SetResult` and `GetResult`.
#[macro_export]
macro_rules! ww_unimplemented {
    () => {
        wire_weaver::Unimplemented.into()
    };
}

/// What a handler of an item declared without a `Result` answers: the value itself, or `Result<T, CommonError>` when
/// it can fail with a common error. Generated servers and `ww_client_server::dispatch` take either.
pub trait IntoOutput<'i, T> {
    /// The value, or the common error the item answers with instead.
    fn into_output(self) -> Result<T, CommonError<'i>>;
}

impl<'i, T> IntoOutput<'i, T> for T {
    #[inline(always)]
    fn into_output(self) -> Result<T, CommonError<'i>> {
        Ok(self)
    }
}

impl<'i, T> IntoOutput<'i, T> for Result<T, CommonError<'i>> {
    #[inline(always)]
    fn into_output(self) -> Result<T, CommonError<'i>> {
        self
    }
}

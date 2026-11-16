use ww_api::ww_error::CommonError;

/// Result type that each server handler returns.
/// - `Ready(T)` - serialize T and send it back to a client that is making a request.
/// - `Deferred` - do nothing, result must be sent later using `cx.request_id` provided.
/// - `Unimplemented` - send `ww_client_server::Error::Unimplemented` back to a client.
///     - No need to return sentinel values when handler is not yet ready
///     - Safe default for codegen from CLI
///
/// Note that `T` can itself be `Result<UserOkTy, UserErrTy>`.
///
/// ## Why not use Result<UserTy, WwError>?
/// Confusing Ok(Ok(ok_user_value)), Ok(Err(err_user_value)), Err(ww_err) statements in every handler.
pub enum RpcResult<T> {
    Ready(T),
    Deferred,
    Unimplemented,
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
        }
    }
}

pub enum SetResult<E> {
    Set,
    SetError(E),
    Unimplemented,
}

pub enum GetResult<T, E> {
    Value(T),
    GetError(E),
    Unimplemented,
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

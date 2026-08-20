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

impl<T> Into<RpcResult<T>> for Unimplemented {
    fn into(self) -> RpcResult<T> {
        RpcResult::Unimplemented
    }
}

impl<E> Into<SetResult<E>> for Unimplemented {
    fn into(self) -> SetResult<E> {
        SetResult::Unimplemented
    }
}

impl<T, E> Into<GetResult<T, E>> for Unimplemented {
    fn into(self) -> GetResult<T, E> {
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

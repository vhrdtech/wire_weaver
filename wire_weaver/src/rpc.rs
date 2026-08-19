/// Result type that each server handler returns.
/// - `Ready(T)` - serialize T and send it back to a client that is making a request.
/// - `Deferred` - do nothing, result must be sent manually later using `cx.request_id` provided.
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

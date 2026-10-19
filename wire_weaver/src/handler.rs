//! Checks the generated server puts around handler calls (SRV-21), so that a handler written as `fn` where
//! `handler_model` says `async` (or the other way round) is a compile error that says so, not a future that is
//! dropped without running. Both are identity functions: no code.
use crate::{GetResult, RpcResult, SetResult};
use core::future::Future;

/// What a sync handler returns. A handler that is `async fn` returns a future instead, which is refused here.
#[diagnostic::on_unimplemented(
    message = "this handler is `async fn`, but `handler_model` (or `use_async = false`) makes it a plain `fn`",
    label = "returns a future, the generated server expects `{Self}` right away",
    note = "make the handler a plain `fn`, or list it as `async` in `ww_codegen!(.., handler_model = \"..\")`"
)]
pub trait SyncReturn {}

impl<T> SyncReturn for RpcResult<T> {}
impl<E> SyncReturn for SetResult<E> {}
impl<T, E> SyncReturn for GetResult<T, E> {}
impl<T> SyncReturn for Option<T> {}
impl SyncReturn for () {}
impl<T: ?Sized> SyncReturn for &mut T {}
impl<T: ?Sized> SyncReturn for &T {}

/// What an async handler returns.
#[diagnostic::on_unimplemented(
    message = "this handler is a plain `fn`, but `handler_model` (or `use_async = true`) makes it `async fn`",
    label = "`{Self}` is not a future",
    note = "make the handler `async fn`, or list it as `sync` in `ww_codegen!(.., handler_model = \"..\")`"
)]
pub trait AsyncReturn: Future {}

impl<F: Future> AsyncReturn for F {}

/// Wraps the call of a sync handler in generated code.
///
/// ```
/// use wire_weaver::{RpcResult, handler};
/// fn ready() -> RpcResult<u8> { RpcResult::Ready(1) }
/// let _ = handler::sync(ready());
/// ```
///
/// An `async fn` where a sync handler is expected doesn't compile:
///
/// ```compile_fail
/// use wire_weaver::{RpcResult, handler};
/// async fn ready() -> RpcResult<u8> { RpcResult::Ready(1) }
/// let _ = handler::sync(ready());
/// ```
#[inline(always)]
pub fn sync<T: SyncReturn>(value: T) -> T {
    value
}

/// Wraps the call of an async handler in generated code, before its `.await`.
///
/// A plain `fn` where an async handler is expected doesn't compile:
///
/// ```compile_fail
/// use wire_weaver::{RpcResult, handler};
/// fn ready() -> RpcResult<u8> { RpcResult::Ready(1) }
/// async fn dispatch() { let _ = handler::r#async(ready()).await; }
/// ```
#[inline(always)]
pub fn r#async<F: AsyncReturn>(future: F) -> F {
    future
}

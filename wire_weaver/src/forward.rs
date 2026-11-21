//! A call through an API's local trait as a handler's answer (SRV-49): what the handlers a server generated with
//! `ww_codegen!(.., serve_trait = "..")` return. The value is the answer, the API's error is written as the API's
//! error, every other error becomes a [Failure] (`Denied`, `DeadlineExceeded`, `Cancelled`, `Overloaded`,
//! `Unimplemented`, `Internal` or the common error the call answered with).

use crate::call::{CallError, Error};
use crate::{Failure, GetResult, RpcResult, SetResult};

/// A method's answer from the call of a method declared without a `Result`.
pub fn call<T>(result: Result<T, Error>) -> RpcResult<T> {
    match result {
        Ok(value) => RpcResult::Ready(value),
        Err(e) => RpcResult::Failed(Failure::from(e)),
    }
}

/// A method's answer from the call of a method declared `-> Result<T, E>`: `E` is the API's error.
pub fn call_result<'i, T, E>(
    result: Result<T, CallError<E>>,
) -> RpcResult<Result<T, crate::Error<'i, E>>> {
    match result {
        Ok(value) => RpcResult::Ready(Ok(value)),
        Err(e) => match e.into_api() {
            Ok(api) => RpcResult::Ready(Err(crate::Error::Api(api))),
            Err(e) => RpcResult::Failed(Failure::from(e)),
        },
    }
}

/// A stream function's answer from the call of one declared `-> Result<T, E>`: the open's output, and the
/// items of the stream, or the API's error `E` refusing the open.
pub fn open_result<'i, T, E, S>(
    result: Result<Result<(T, S), E>, Error>,
) -> (RpcResult<Result<T, crate::Error<'i, E>>>, Option<S>) {
    match result {
        Ok(Ok((value, items))) => (RpcResult::Ready(Ok(value)), Some(items)),
        Ok(Err(api)) => (RpcResult::Ready(Err(crate::Error::Api(api))), None),
        Err(e) => (RpcResult::Failed(Failure::from(e)), None),
    }
}

/// A property read's answer.
pub fn read<T, E>(result: Result<T, Error>) -> GetResult<T, E> {
    match result {
        Ok(value) => GetResult::Value(value),
        Err(e) => GetResult::Failed(Failure::from(e)),
    }
}

/// A property write's answer, of a property without an error type of its own.
pub fn write<E>(result: Result<(), Error>) -> SetResult<E> {
    match result {
        Ok(()) => SetResult::Set,
        Err(e) => SetResult::Failed(Failure::from(e)),
    }
}

/// A property write's answer, of a property declared with the error type `E`.
pub fn write_result<'i, E>(result: Result<(), CallError<E>>) -> SetResult<crate::Error<'i, E>> {
    match result {
        Ok(()) => SetResult::Set,
        Err(e) => match e.into_api() {
            Ok(api) => SetResult::SetError(crate::Error::Api(api)),
            Err(e) => SetResult::Failed(Failure::from(e)),
        },
    }
}

/// The items of a stream until its first error: an error is the stream's last item ([crate::call::ItemStream],
/// [crate::call::LocalItemStream]), so the server's pump ends there. `Send` when `items` is.
pub fn until_error<T, S>(items: S) -> impl futures_core::Stream<Item = T> + use<T, S>
where
    S: futures_core::Stream<Item = Result<T, Error>> + Unpin,
{
    UntilError { items: Some(items) }
}

struct UntilError<S> {
    items: Option<S>,
}

impl<T, S> futures_core::Stream for UntilError<S>
where
    S: futures_core::Stream<Item = Result<T, Error>> + Unpin,
{
    type Item = T;

    fn poll_next(
        mut self: core::pin::Pin<&mut Self>,
        cx: &mut core::task::Context<'_>,
    ) -> core::task::Poll<Option<T>> {
        use core::task::Poll;
        let Some(items) = self.items.as_mut() else {
            return Poll::Ready(None);
        };
        match core::pin::Pin::new(items).poll_next(cx) {
            Poll::Ready(Some(Ok(item))) => Poll::Ready(Some(item)),
            Poll::Ready(Some(Err(_)) | None) => {
                self.items = None;
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call::ErrorKind;

    #[test]
    fn api_error_stays_the_apis() {
        let r: RpcResult<Result<u8, crate::Error<'_, u8>>> = call_result(Err(CallError::Api(3u8)));
        assert!(matches!(r, RpcResult::Ready(Err(crate::Error::Api(3)))));
    }

    #[test]
    fn other_errors_are_failures() {
        let r: RpcResult<Result<u8, crate::Error<'_, u8>>> =
            call_result(Err(CallError::Denied(None)));
        let RpcResult::Failed(f) = r else {
            panic!("a failure")
        };
        assert_eq!(f.error().kind(), ErrorKind::Denied);
        let r: GetResult<u8, ()> = read(Err(Error::internal("broken")));
        assert!(matches!(r, GetResult::Failed(_)));
        let r: SetResult<()> = write(Ok(()));
        assert!(matches!(r, SetResult::Set));
    }

    #[test]
    fn common_error_is_borrowed_back() {
        use ww_api::ww_error::CommonErrorOwned;
        let f = Failure::from(Error::common(CommonErrorOwned::Busy));
        assert!(matches!(f.common(), Some(crate::CommonError::Busy)));
        assert!(Failure::from(Error::denied("no")).common().is_none());
    }
}

//! Per-item error lists on the client (`#[errors(..)]` on methods, stream functions and properties, API-32): a
//! device returning an error outside an item's list is logged, see `docs/api/errors.md#which-errors-a-method-returns`. Nothing on the device checks the list.

use crate::error::OutcomeError;
use std::fmt::Debug;
use std::sync::atomic::{AtomicBool, Ordering};
use ww_error::CommonErrorOwned;

static STRICT: AtomicBool = AtomicBool::new(false);

/// An item's `#[errors(..)]` list, as generated client code gives it to
/// [PreparedCall::with_error_list](crate::PreparedCall::with_error_list) (`E` is the call's
/// [CallError](crate::CallError)), [PreparedWrite::with_error_list](crate::PreparedWrite::with_error_list) (a
/// property write, `CallError` too) and
/// [PreparedStreamOpen::with_error_list](crate::PreparedStreamOpen::with_error_list) (`E` is the stream function's
/// error type itself).
pub struct ErrorList<E> {
    /// False for a device error outside the list ([CallError::Device](crate::CallError::Device) of a call), true for
    /// every other error.
    pub listed: fn(&E) -> bool,
    /// False for a common error outside the list's `CommonError(..)` entry (STD-16); true for every common error
    /// when the list has no such entry.
    pub common: fn(&CommonErrorOwned) -> bool,
    /// The list as written in the trait, `CommonError(InUse, Refused), Nack`.
    pub names: &'static str,
}

impl<E> Clone for ErrorList<E> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<E> Copy for ErrorList<E> {}

/// Panic instead of logging a warning when a device returns an error outside an item's `#[errors(..)]` list, for
/// the whole process. Meant for tests (`tests_common` turns it on): a device that breaks its API's lists fails them.
pub fn set_strict_error_lists(strict: bool) {
    STRICT.store(strict, Ordering::Relaxed);
}

/// Logs (or in [strict](set_strict_error_lists) mode panics) when `error`, the outcome of a call or a write, is
/// not in the `list` of `resource`: a common error against the list's `CommonError(..)` entry, any other against
/// its own entries.
pub(crate) fn check<E: OutcomeError>(error: &E, list: Option<&ErrorList<E>>, resource: &str) {
    match error.common_error() {
        Some(c) => check_common(c, list, resource),
        None => check_api(error, list, resource),
    }
}

/// [check] of an error of the API's own: a stream function's `Err`.
pub(crate) fn check_api<E: Debug>(error: &E, list: Option<&ErrorList<E>>, resource: &str) {
    if let Some(list) = list
        && !(list.listed)(error)
    {
        unlisted(resource, &format!("{error:?}"), list.names);
    }
}

/// [check] of a common error.
pub(crate) fn check_common<E>(
    error: &CommonErrorOwned,
    list: Option<&ErrorList<E>>,
    resource: &str,
) {
    if let Some(list) = list
        && !(list.common)(error)
    {
        unlisted(
            resource,
            &format!("CommonError({})", error.name()),
            list.names,
        );
    }
}

/// Reports a device error outside the `#[errors(..)]` list of `resource`, see [set_strict_error_lists].
pub(crate) fn unlisted(resource: &str, error: &str, names: &str) {
    if STRICT.load(Ordering::Relaxed) {
        panic!("'{resource}' returned {error}, which is not in its #[errors({names})]");
    }
    tracing::warn!("'{resource}' returned {error}, which is not in its #[errors({names})]");
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIST: ErrorList<u8> = ErrorList {
        listed: |e| *e < 2,
        common: |c| matches!(c, CommonErrorOwned::Busy),
        names: "CommonError(Busy), Zero, One",
    };

    #[test]
    fn warns_or_panics() {
        set_strict_error_lists(false);
        check_api(&1, Some(&LIST), "T::m");
        check_api(&5, Some(&LIST), "T::m"); // only a warning
        check_api(&5, None, "T::m");
        set_strict_error_lists(true);
        check_api(&1, Some(&LIST), "T::m");
        let e = std::panic::catch_unwind(|| check_api(&5, Some(&LIST), "T::m")).unwrap_err();
        assert_eq!(
            e.downcast_ref::<String>().unwrap(),
            "'T::m' returned 5, which is not in its #[errors(CommonError(Busy), Zero, One)]"
        );
        set_strict_error_lists(false);
    }

    /// A common error is checked against the list's `CommonError(..)` entry, by its name (STD-16).
    #[test]
    fn common_errors_by_name() {
        set_strict_error_lists(true);
        check_common(&CommonErrorOwned::Busy, Some(&LIST), "T::m");
        check_common::<u8>(&CommonErrorOwned::Timeout, None, "T::m");
        let e = std::panic::catch_unwind(|| {
            check_common(&CommonErrorOwned::Timeout, Some(&LIST), "T::m")
        })
        .unwrap_err();
        assert_eq!(
            e.downcast_ref::<String>().unwrap(),
            "'T::m' returned CommonError(Timeout), which is not in its #[errors(CommonError(Busy), Zero, One)]"
        );
        set_strict_error_lists(false);
    }
}

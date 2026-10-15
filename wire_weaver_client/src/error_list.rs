//! Per-item error lists on the client (`#[errors(..)]` on methods, stream functions and properties, API-32): a
//! device returning an error outside an item's list is logged, see `docs/api/errors.md#which-errors-a-method-returns`. Nothing on the device checks the list.

use std::fmt::Debug;
use std::sync::atomic::{AtomicBool, Ordering};

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
    /// The list as written in the trait, `Common(InUse, Refused), Nack`.
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

/// Logs (or in [strict](set_strict_error_lists) mode panics) when `error` is not in the `list` of `resource`.
pub(crate) fn check<E: Debug>(error: &E, list: Option<&ErrorList<E>>, resource: &str) {
    if let Some(list) = list
        && !(list.listed)(error)
    {
        unlisted(resource, &format!("{error:?}"), list.names);
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

    #[test]
    fn warns_or_panics() {
        let list = ErrorList::<u8> {
            listed: |e| *e < 2,
            names: "Zero, One",
        };
        set_strict_error_lists(false);
        check(&1, Some(&list), "T::m");
        check(&5, Some(&list), "T::m"); // only a warning
        check(&5, None, "T::m");
        set_strict_error_lists(true);
        check(&1, Some(&list), "T::m");
        let e = std::panic::catch_unwind(|| check(&5, Some(&list), "T::m")).unwrap_err();
        assert_eq!(
            e.downcast_ref::<String>().unwrap(),
            "'T::m' returned 5, which is not in its #[errors(Zero, One)]"
        );
        set_strict_error_lists(false);
    }
}

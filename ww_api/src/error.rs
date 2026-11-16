//! What a handler answers instead of a value: a common error every item can return, or the API's own.

use ww_error::CommonError;

/// What a handler's call failed with: a common error every item can return, or the API's own error `E`.
///
/// A handler of a method `fn read() -> Result<u16, SensorError>` returns
/// `Result<u16, Error<'_, SensorError>>`: `?` takes a [CommonError] from any helper, and a `SensorError` once
/// its crate declared [`api_error!(SensorError)`](crate::api_error). A method declared without a `Result`
/// returns `Result<T, CommonError>` instead, there is no API error to hold.
///
/// Protocol-neutral: a dispatcher sends `Common` as its protocol's common error and `Api` as the method's `Err`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error<'i, E> {
    /// An error every item can return, sent by the protocol beside the API's own.
    Common(CommonError<'i>),
    /// The API's own error, the `Err` of the method's `Result`.
    Api(E),
}

impl<'i, E> Error<'i, E> {
    /// The common error, `None` for the API's own.
    pub fn common(&self) -> Option<&CommonError<'i>> {
        match self {
            Error::Common(c) => Some(c),
            Error::Api(_) => None,
        }
    }

    /// The API's own error, `None` for a common one.
    pub fn api(&self) -> Option<&E> {
        match self {
            Error::Common(_) => None,
            Error::Api(e) => Some(e),
        }
    }

    /// The API's error through `f`, a common one as it is.
    pub fn map_api<F>(self, f: impl FnOnce(E) -> F) -> Error<'i, F> {
        match self {
            Error::Common(c) => Error::Common(c),
            Error::Api(e) => Error::Api(f(e)),
        }
    }
}

impl<'i, E> From<CommonError<'i>> for Error<'i, E> {
    fn from(c: CommonError<'i>) -> Self {
        Error::Common(c)
    }
}

/// `From<XError>` for [Error], so a handler's `?` takes the API's own error: one line per error enum, in its crate.
///
/// A blanket `From<E>` would overlap with `From<CommonError>` at `E = CommonError`, so each error enum names
/// itself, with its lifetime, and the owned type where the crate makes one:
///
/// ```
/// use ww_api::prelude::*;
///
/// #[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone), ww_repr = unib32)]
/// pub enum SensorError {
///     /// The sensor did not answer.
///     Nack,
///     /// The sensor's reply was {reason}.
///     Garbled { reason: MaxLen<&'i str, 16> },
/// }
/// ww_api::api_error!(SensorError);
/// #[cfg(feature = "std")]
/// ww_api::api_error!(SensorError);
///
/// fn read() -> Result<u16, Error<'static, SensorError>> {
///     Err(SensorError::Nack)?
/// }
/// assert_eq!(read().unwrap_err().api(), Some(&SensorError::Nack));
/// ```
#[macro_export]
macro_rules! api_error {
    ($ty:ident<$lt:lifetime>) => {
        impl<$lt> ::core::convert::From<$ty<$lt>> for $crate::Error<$lt, $ty<$lt>> {
            fn from(e: $ty<$lt>) -> Self {
                $crate::Error::Api(e)
            }
        }
    };
    ($ty:ident) => {
        impl<'i> ::core::convert::From<$ty> for $crate::Error<'i, $ty> {
            fn from(e: $ty) -> Self {
                $crate::Error::Api(e)
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq)]
    enum ApiErr {
        Nack,
    }
    api_error!(ApiErr);

    #[test]
    fn question_mark_takes_both() {
        fn common() -> Result<(), CommonError<'static>> {
            Err(CommonError::Busy)
        }
        fn api() -> Result<(), ApiErr> {
            Err(ApiErr::Nack)
        }
        fn handler(pick: bool) -> Result<u8, Error<'static, ApiErr>> {
            if pick {
                common()?
            } else {
                api()?
            }
            Ok(1)
        }
        assert_eq!(
            handler(true).unwrap_err().common(),
            Some(&CommonError::Busy)
        );
        assert_eq!(handler(false).unwrap_err().api(), Some(&ApiErr::Nack));
        let e: Error<'_, ApiErr> = Error::Api(ApiErr::Nack);
        assert_eq!(e.map_api(|_| 7u8), Error::Api(7));
    }
}

//! Test API of `tests/methods`: one method per dispatch shape (plain, user-defined types, deferred, bit-sized
//! arguments, fallible, context-visible seq/medium), to test method dispatch end to end.
use ww_api::prelude::*;

#[ww_api_root]
#[effect(edit)]
trait Methods {
    fn no_args();
    fn one_plain_arg(value: u8);
    fn plain_return() -> u8;
    fn user_arg(u: UserDefined<'i>);
    fn user_defined_return() -> UserDefined<'i>;
    /// Server returns Unimplemented
    fn absent();
    /// Answered later from outside of the handler, or never
    fn deferred() -> u8;
    fn deferred_unit();
    /// Answers pending `deferred` and `deferred_unit` calls from the handler, with `value` for `deferred`
    fn answer_deferred(value: u8);
    /// Sequence number of this request, as seen by the handler
    fn request_seq() -> u32;
    /// Medium this request came from, as seen by the handler
    fn request_medium() -> u8;
    /// Bit-sized arguments and return (SW-48)
    fn sub_byte(a: U7, b: I10, addr: I2cAddress) -> U10;
    /// `value / 2`, or `Err(MethodError::Odd)` for an odd value
    #[errors(Odd)]
    fn fallible(value: u8) -> Result<u8, MethodError>;
    /// Errors from a list (API-32): `pick` 0 is `Ok`, 1 `CommonError::Busy`, 2 `Nack`, 3 `CommonError::Timeout` and
    /// 4 `Overrun`, which are not in the list
    #[errors(CommonError(Busy, InUse), Nack)]
    fn listed_errors(pick: u8) -> Result<(), ListedError>;
    /// Any common error (API-32): `CommonError` alone lists all of them
    #[errors(CommonError)]
    fn all_common(pick: u8) -> Result<(), ListedError>;
    /// Server returns Unimplemented
    fn fallible_absent() -> Result<(), MethodError>;
    /// Array of units (CLT-15 multi-request tests: list/range/mask indices through `call_array`)
    ww_impl!(unit[]: Unit);
    /// Nested level with a deferred method (answered by `answer_deferred`, handler `plug_vbus_cycle`)
    ww_impl!(plug: Plug);
}

#[ww_trait(local = "PlugLocal")]
#[effect(edit)]
trait Plug {
    /// Answered later with the generated `plug_vbus_cycle_send_return`
    fn vbus_cycle() -> u8;
}

#[ww_trait(local = "UnitLocal")]
#[effect(edit)]
trait Unit {
    /// `value` doubled, wrapping
    fn double(value: u8) -> u8;
    /// `value / 2`, or `Err(MethodError::Odd)` for an odd value (CLT-48: fallible methods in `call_array`)
    #[errors(Odd)]
    fn halve(value: u8) -> Result<u8, MethodError>;
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq), ww_repr = u4)]
pub enum MethodError {
    Odd,
}

ww_api::api_error!(MethodError);

impl core::fmt::Display for MethodError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            MethodError::Odd => write!(f, "odd value"),
            MethodError::Unknown(n) => write!(f, "unknown error {n}"),
        }
    }
}

impl core::error::Error for MethodError {}

/// Error of [Methods::listed_errors]: the common errors first, then the API's own (STD-16).
#[derive_shrink_wrap(
    borrowed, owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone),
    ww_repr = unib32,
    display
)]
pub enum ListedError {
    /// The sensor did not answer.
    Nack,
    /// Data was lost.
    Overrun,
}

ww_api::api_error!(ListedError);

#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Eq))]
struct UserDefined<'i> {
    a: u8,
    b: RefVec<'i, u8>,
}

/// 7-bit or 10-bit I2C address, 8 or 11 bits on the wire (SW-48)
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Copy, Clone),
    ww_repr = u1,
    self_describing
)]
pub enum I2cAddress {
    Seven(U7),
    Ten(U10),
}

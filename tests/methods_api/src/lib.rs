use wire_weaver::prelude::*;

#[ww_trait]
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
    fn fallible(value: u8) -> Result<u8, MethodError>;
    /// Server returns Unimplemented
    fn fallible_absent() -> Result<(), MethodError>;
}

#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq), ww_repr = u4)]
pub enum MethodError {
    Odd,
}

impl core::fmt::Display for MethodError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            MethodError::Odd => write!(f, "odd value"),
            MethodError::Unknown(n) => write!(f, "unknown error {n}"),
        }
    }
}

impl core::error::Error for MethodError {}

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

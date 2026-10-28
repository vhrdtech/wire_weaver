#![cfg_attr(not(feature = "std"), no_std)]
//! Units of measure fixed at compile time, exact: a number with its unit, scale and storage in the type, on the wire
//! exactly the number.
//!
//! - [Fixed]`<U, P, T, B>`: a `T` in unit `U` and scale `P` (a prefix, a binary prefix, a power of ten, a fraction),
//!   within bounds `B`. `Fixed<Second, Micro, u32>` is a time in µs, the same bytes as the `u32` it replaces.
//!   Conversions between scales are exact or refused, never rounded through a float.
//! - [units]: zero-size unit markers (SI, angle, information, counted things) with their [Dim] and [Scale], and the
//!   products and quotients the compiler checks (`Volt · Ampere` is `Watt`).
//! - [Dim]: the dimension as data: SI base exponents, angle, information, up to [MAX_COUNTED] counted bases and a
//!   square root (V/√Hz). [Magnitude]: a factor of integers and powers of 10, 2 and π (`°` is π/180 rad).
//! - `SIExpr` (feature `shrink_wrap`): a unit as a tree on the wire, for a unit only known at run time.
//!
//! `no_std`, no alloc, and no dependencies without features. `shrink_wrap` adds the wire format (`shrink_wrap`'s),
//! `std` every unit group and with `shrink_wrap` the owned forms of the wire types, `defmt` their `defmt::Format`. Unit
//! groups are cargo features (`time`, `electrical`, `mechanical`, `angle`, `thermal`, `chemical`, `photometric`,
//! `radiation`, `dimensionless`, `information`, `counting`, `quantities` for all).
//!
//! Run-time quantities (a prefix and a number type chosen per value) are `ww_si`'s `Quantity` and `AnyQuantity`,
//! built on this crate.

// unit tests format with std also when the crate is built without it
#[cfg(all(test, not(feature = "std")))]
extern crate std;

mod declare;
mod fixed;
mod magnitude;
pub mod num;
mod products;
#[cfg(feature = "shrink_wrap")]
mod si_expr;
mod si_prefix;
pub mod tree;
mod unit;
pub mod units;

#[cfg(feature = "information")]
pub use fixed::Bytes;
#[cfg(feature = "angle")]
pub use fixed::{Bam16, Bam32};
pub use fixed::{
    Bounds, DecimalPrefix, Fixed, FixedScale, OutOfBounds, Range, RangeDec, RangeU64, Repr,
    Storage, Unbounded, UnsignedStorage, prefix,
};
#[cfg(feature = "dimensionless")]
pub use fixed::{Duty16, Ppb, Q15, Q31, UQ16_16};
#[cfg(feature = "time")]
pub use fixed::{Micros, Millis, Nanos};
pub use magnitude::{
    Irrational, MAX_FACTORS, Magnitude, MagnitudeError, MagnitudeRest, PrimePower,
};
#[cfg(feature = "shrink_wrap")]
pub use si_expr::SIExpr;
#[cfg(all(feature = "std", feature = "shrink_wrap"))]
pub use si_expr::SIExprOwned;
pub use si_prefix::Prefix;
pub use tree::MAX_UNIT_DEPTH;
#[doc(hidden)]
pub use unit::exp as __dim_exp;
pub use unit::{Counted, Dim, MAX_COUNTED, Scale, Unit, UnitDiv, UnitExpr, UnitMul};
#[cfg(all(feature = "std", feature = "shrink_wrap"))]
pub use unit::{CountedOwned, DimOwned};

/// The [Dim] of a counted unit marker `Name` of the crate this is written in: `Dim::counted(<this crate>, "Name")`,
/// with the crate as Rust names it (`CARGO_CRATE_NAME`). Two crates' markers of the same name are different bases.
///
/// ```
/// let bucket = exact_units::counted!(Bucket);
/// assert_eq!(bucket, exact_units::Dim::counted(env!("CARGO_CRATE_NAME"), "Bucket"));
/// assert_eq!(bucket.to_string(), "Bucket");
/// ```
#[macro_export]
macro_rules! counted {
    ($name:ident) => {
        $crate::Dim::counted(env!("CARGO_CRATE_NAME"), stringify!($name))
    };
}

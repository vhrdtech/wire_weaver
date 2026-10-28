//! One walk over a unit tree ([exact_units::tree]), and a quantity's `Display` on top of it: written once for a typed
//! [Quantity](crate::Quantity), an [AnyQuantity](crate::AnyQuantity) and a client's own unit model
//! (`wire_weaver_client`'s `DynUnit`), so a unit known at run time or only from introspection data is shown exactly
//! like the same unit known from a type.
//!
//! ```
//! use ww_si::tree::{self, Node, Tree};
//! use ww_si::Prefix;
//! use ww_si::ww_numeric::NumericValue;
//!
//! /// A unit as some client keeps it: names it only knows, and quotients of them.
//! enum MyUnit {
//!     Name(&'static str),
//!     Per(Box<MyUnit>, Box<MyUnit>),
//! }
//!
//! impl<'a> Tree<'a> for &'a MyUnit {
//!     fn node(&self) -> Option<Node<'a, Self>> {
//!         Some(match *self {
//!             MyUnit::Name(n) => Node::Named { name: n, symbol: n, dim: None, scale: None },
//!             MyUnit::Per(a, b) => Node::Div(&**a, &**b),
//!         })
//!     }
//! }
//!
//! let u = MyUnit::Per(Box::new(MyUnit::Name("rpm")), Box::new(MyUnit::Name("s")));
//! assert_eq!(tree::symbol(&&u).to_string(), "rpm/s");
//! let v = NumericValue::U8(12);
//! assert_eq!(tree::quantity(Prefix::Kilo, &v, &&u).to_string(), "12 k(rpm/s)");
//! ```

pub use exact_units::tree::*;

use crate::Prefix;
#[cfg(feature = "f32")]
use crate::ops::scale_f32;
use crate::ops::scale_f64;
use core::fmt;
use ww_numeric::NumericValue;

/// A quantity as `Display` writes it: `3300 mV`, the number as stored with its prefix; `{:#}` writes it in the unit
/// itself, `3.3 V`. The text of a typed [Quantity](crate::Quantity) of the same unit.
pub fn quantity<'a, 't, E: Tree<'t>>(
    prefix: Prefix,
    value: &'a NumericValue,
    unit: &'a E,
) -> impl fmt::Display + 'a {
    struct Q<'a, E>(Prefix, &'a NumericValue, &'a E);
    impl<'t, E: Tree<'t>> fmt::Display for Q<'_, E> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            let Q(prefix, value, unit) = *self;
            if f.alternate() {
                match *value {
                    #[cfg(feature = "f32")]
                    NumericValue::F32(x) => write!(f, "{}", scale_f32(x, prefix.into()))?,
                    _ => write!(f, "{}", scale_f64(value.to_f64(), prefix.into()))?,
                }
                write_unit(Prefix::Unit, unit, f)
            } else {
                write_number(value, f)?;
                write_unit(prefix, unit, f)
            }
        }
    }
    Q(prefix, value, unit)
}

/// The number as stored: integers exactly, `f32` as `f32` (3.3, not 3.299999952316284).
pub(crate) fn write_number(value: &NumericValue, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match *value {
        #[cfg(feature = "f32")]
        NumericValue::F32(x) => write!(f, "{x}"),
        #[cfg(feature = "f64")]
        NumericValue::F64(x) => write!(f, "{x}"),
        value => match value.to_integer() {
            Some(x) => write!(f, "{x}"),
            // UN / IN
            None => write!(f, "{}", value.to_f64()),
        },
    }
}

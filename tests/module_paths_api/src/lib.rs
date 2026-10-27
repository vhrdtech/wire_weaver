//! Types in modules (API-46): generated code names each one by the path other crates see, not `<crate>::<Name>`. A
//! type in a `pub mod` that isn't re-exported, one with a lifetime two modules deep (its `LabelOwned` on the client),
//! one imported under another name (`use .. as ..`), one in a private module re-exported at the root, one re-exported
//! under another name, two equal definitions with the same name in different modules, a unit marker
//! `units::Volt` next to a root `Volt` of another shape, and a type named `Range` (like `ww_si::Range`) in a private
//! module's `pub mod` re-exported by `pub use`, next to a `core::ops::Range`.
#![cfg_attr(not(feature = "std"), no_std)]

use core::marker::PhantomData;
use core::ops::Range;
use geometry::Point as Spot;
use wire_weaver::prelude::*;

#[ww_trait]
trait ModulePaths {
    /// The point mirrored at the origin
    fn mirror(p: Spot) -> geometry::Point;
    /// The point moved by one along z
    fn lift(p: other::Point) -> other::Point;
    /// Length of the label's text
    fn measure(label: geometry::shapes::Label<'i>) -> u32;
    /// The device's name
    fn name() -> geometry::shapes::Label<'i>;
    /// The quantity as a plain `Volt`
    fn convert(q: Quantity<units::Volt>) -> Volt;
    property!(rw hidden: Hidden);
    property!(rw renamed: Renamed);
    /// `x` limited to `0..=10`, then moved by `shift.start`
    fn clamp(shift: Range<i32>, x: limits::Range<0, 10>) -> limits::Range<0, 10>;
}

pub mod geometry {
    use wire_weaver::prelude::*;

    #[derive_shrink_wrap(
        borrowed,
        owned(feature = "std"),
        derive(Debug, PartialEq, Clone, Copy)
    )]
    pub struct Point {
        pub x: i16,
        pub y: i16,
    }

    pub mod shapes {
        use wire_weaver::prelude::*;

        #[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Clone))]
        pub struct Label<'i> {
            pub text: &'i str,
        }
    }
}

/// Equal to `geometry::Point`, a type of its own
pub mod other {
    use wire_weaver::prelude::*;

    #[derive_shrink_wrap(
        borrowed,
        owned(feature = "std"),
        derive(Debug, PartialEq, Clone, Copy)
    )]
    pub struct Point {
        pub x: i16,
        pub y: i16,
    }
}

mod hidden {
    use wire_weaver::prelude::*;

    #[derive_shrink_wrap(
        borrowed,
        owned(feature = "std"),
        derive(Debug, PartialEq, Clone, Copy)
    )]
    pub struct Hidden {
        pub id: u8,
    }

    #[derive_shrink_wrap(
        borrowed,
        owned(feature = "std"),
        derive(Debug, PartialEq, Clone, Copy)
    )]
    pub struct Inner {
        pub id: u32,
    }
}

mod private {
    /// A module other crates name as `limits`, through the `pub use` at the root
    pub mod limits {
        use wire_weaver::prelude::*;

        /// A value within `MIN..=MAX`, named like `core::ops::Range`
        #[derive_shrink_wrap(
            borrowed,
            owned(feature = "std"),
            derive(Debug, PartialEq, Clone, Copy)
        )]
        pub struct Range<const MIN: i32, const MAX: i32> {
            pub value: i32,
        }
    }
}

pub use private::limits;

pub use hidden::Hidden;
pub use hidden::Inner as Renamed;

/// A value of the unit `U`, the unit only in the type
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Clone, Copy)
)]
pub struct Quantity<U> {
    pub value: i32,
    _unit: PhantomData<U>,
}

impl<U> Quantity<U> {
    pub const fn new(value: i32) -> Self {
        Self {
            value,
            _unit: PhantomData,
        }
    }
}

/// Volts in millivolts, named like the marker in `units`
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Clone, Copy)
)]
pub struct Volt {
    pub mv: i32,
}

pub mod units {
    /// The unit marker, no serdes of its own
    #[derive(Debug, PartialEq, Clone, Copy)]
    pub struct Volt;
}

//! [unit!](crate::unit) and [scale!](crate::scale): unit and scale markers declared as data, so that both the
//! compiler (through the `impl Unit` / `impl FixedScale` they expand to) and WireWeaver's walker (which reads the
//! same lines from source, INT-13) know what a marker stands for. A hand-written `impl Unit` still compiles, but an
//! API that uses such a marker is refused by the walker: a client would not know the unit.

/// Declares unit markers: a zero-size struct per line and its [Unit](crate::Unit) impl, from the unit's symbol,
/// dimension and scale. WireWeaver reads the same lines from source, so a client of an API that uses the marker
/// knows its definition without a table of its own.
///
/// ```
/// exact_units::unit! {
///     /// Revolutions per minute: 2π rad / 60 s, exactly.
///     pub struct Rpm = "rpm", [rad 1, s -1], Exact(1 / 30, pi 1);
///     /// A count of buckets of a histogram, a base of its own (named by this crate and `Bucket`).
///     pub struct Bucket = "bucket", counted;
/// }
/// use exact_units::{Fixed, Unit, prefix::One};
///
/// let speed = Fixed::<Rpm, One, u8>::new(60);
/// assert_eq!(format!("{speed}"), "60 rpm");
/// assert!((Rpm::EXPR.factor() * 60.0 - core::f64::consts::TAU).abs() < 1e-12); // rad/s
/// assert_eq!(Bucket::DIM, exact_units::counted!(Bucket));
/// ```
///
/// Each line is `<attributes> <visibility> struct <Name> = "<symbol>", <dimension>[, <scale>];`:
///
/// - The attributes are the marker's (docs, `#[cfg(..)]`); its derives are `Copy, Clone, Debug, Default, PartialEq,
///   Eq, Hash`. A `#[cfg(..)]` applies to its impl too.
/// - The dimension is a list of exponents of `s`, `m`, `kg`, `a`, `k`, `mol`, `cd`, `rad`, `bit` (`[kg 1, m 2, s -3,
///   a -1]` for a volt, `[]` for a ratio), or `counted`: a counted base of its own named by the crate and the marker
///   ([counted!](crate::counted)).
/// - The scale, coherent when left out: `Pow10(-3)`, `Exact(1 / 180, pi 1)` (`num / den`, then optionally `pow10 e`,
///   `pow2 e`, `pi e` in that order, see [Magnitude::new](crate::Magnitude::new)), `Factor(1.602176634e-19)` or
///   `Affine { factor: 1.0, offset: 273.15 }`; see [Scale](crate::Scale). Prefer `Exact` over `Factor`: every reader
///   computes the same value.
///
/// Literal values only: the walker reads them from source, it doesn't evaluate constants.
#[macro_export]
macro_rules! unit {
    // exact_units' own table: the markers and a static of all their `UnitExpr`s
    (
        @table $(#[$all_attr:meta])* $all_vis:vis static $all:ident;
        $(
            $(#[$($attr:tt)*])*
            $vis:vis struct $name:ident = $symbol:literal, $dim:tt $(, $kind:ident $args:tt)?;
        )*
    ) => {
        $crate::unit! {
            $(
                $(#[$($attr)*])*
                $vis struct $name = $symbol, $dim $(, $kind $args)?;
            )*
        }

        $(#[$all_attr])*
        #[allow(unused_doc_comments)]
        $all_vis static $all: &[&$crate::UnitExpr] = &[
            $(
                $(#[$($attr)*])*
                <$name as $crate::Unit>::EXPR,
            )*
        ];
    };
    (
        $(
            $(#[$($attr:tt)*])*
            $vis:vis struct $name:ident = $symbol:literal, $dim:tt $(, $kind:ident $args:tt)?;
        )*
    ) => {
        $(
            $(#[$($attr)*])*
            #[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
            $vis struct $name;

            $crate::__with_cfgs! {
                [] $(#[$($attr)*])* =>
                impl $crate::Unit for $name {
                    const DIM: $crate::Dim<'static> = $crate::__unit_dim!($name $dim);
                    const EXPR: &'static $crate::UnitExpr = &$crate::UnitExpr::Named {
                        name: stringify!($name),
                        symbol: $symbol,
                        dim: Self::DIM,
                        scale: $crate::__unit_scale!($($kind $args)?),
                    };
                }
            }
        )*
    };
}

/// Declares scale markers, the `P` of a [Fixed](crate::Fixed): a zero-size struct per line and its
/// [FixedScale](crate::FixedScale) impl, from its symbol and exact factor. WireWeaver reads the same lines from
/// source, like [unit!](crate::unit)'s.
///
/// ```
/// exact_units::scale! {
///     /// One step of a gyro, 70 mdps per LSB: 70/1000 of the unit.
///     pub struct Mdps70 = "", Exact(70 / 1000);
///     /// A quarter, 2⁻².
///     pub struct Quarter = "", Pow2(-2);
/// }
/// use exact_units::{FixedScale, Magnitude, prefix::Milli};
///
/// assert_eq!(Mdps70::SCALE, Magnitude::ratio(7, 100));
/// assert_eq!(Quarter::SCALE.to_f64(), 0.25);
/// assert_eq!(Milli::SCALE, Magnitude::pow10(-3));
/// ```
///
/// Each line is `<attributes> <visibility> struct <Name> = "<symbol>", <factor>[, prefix <Prefix>];`: the symbol is
/// written before a unit's (`"m"`, empty for a factor without a prefix); the factor is `Pow10(e)`, `Pow2(e)` or
/// `Exact(num / den, ..)` as in [unit!](crate::unit); `prefix Milli` names the run-time [Prefix](crate::Prefix) of
/// the same scale, and the marker is a [DecimalPrefix](crate::DecimalPrefix) then.
#[macro_export]
macro_rules! scale {
    (
        $(
            $(#[$($attr:tt)*])*
            $vis:vis struct $name:ident = $symbol:literal, $kind:ident $args:tt $(, prefix $prefix:ident)?;
        )*
    ) => {
        $(
            $(#[$($attr)*])*
            #[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
            $vis struct $name;

            $crate::__with_cfgs! {
                [] $(#[$($attr)*])* =>
                impl $crate::FixedScale for $name {
                    const SCALE: $crate::Magnitude = $crate::__scale_magnitude!($kind $args);
                    const SYMBOL: &'static str = $symbol;
                    const NAME: &'static str = stringify!($name);
                    const RUNTIME: Option<$crate::Prefix> = $crate::__scale_runtime!($($prefix)?);
                }
            }
            $crate::__scale_decimal! { [$(#[$($attr)*])*] $name $($prefix)? }
        )*
    };
}

/// `item` with only the `#[cfg(..)]` attributes of the ones before `=>`.
#[doc(hidden)]
#[macro_export]
macro_rules! __with_cfgs {
    ([$($cfg:tt)*] #[cfg $c:tt] $($rest:tt)*) => {
        $crate::__with_cfgs! { [$($cfg)* #[cfg $c]] $($rest)* }
    };
    ([$($cfg:tt)*] #[$($other:tt)*] $($rest:tt)*) => {
        $crate::__with_cfgs! { [$($cfg)*] $($rest)* }
    };
    ([$($cfg:tt)*] => $($item:tt)*) => {
        $($cfg)* $($item)*
    };
}

/// The dimension of a [unit!] line.
#[doc(hidden)]
#[macro_export]
macro_rules! __unit_dim {
    ($name:ident counted) => {
        $crate::counted!($name)
    };
    ($name:ident [$($d:ident $e:literal),* $(,)?]) => {{
        #[allow(unused_mut)]
        let mut dim = $crate::Dim::ONE;
        $(dim.$d = $crate::__dim_exp($e);)*
        dim
    }};
}

/// The scale of a [unit!] line.
#[doc(hidden)]
#[macro_export]
macro_rules! __unit_scale {
    () => {
        $crate::Scale::Coherent
    };
    (Pow10 ($e:literal)) => {
        $crate::Scale::Pow10($e)
    };
    (Factor ($f:literal)) => {
        $crate::Scale::Factor($f)
    };
    (Affine { factor: $f:literal, offset: $o:literal $(,)? }) => {
        $crate::Scale::Affine {
            factor: $f,
            offset: $o,
        }
    };
    (Exact ($($m:tt)*)) => {
        $crate::Scale::Exact($crate::__magnitude!($($m)*))
    };
}

/// The factor of a [scale!] line.
#[doc(hidden)]
#[macro_export]
macro_rules! __scale_magnitude {
    (Pow10 ($e:literal)) => {
        $crate::Magnitude::pow10($e)
    };
    (Pow2 ($e:literal)) => {
        $crate::Magnitude::pow2($e)
    };
    (Exact ($($m:tt)*)) => {
        $crate::__magnitude!($($m)*)
    };
}

/// The [DecimalPrefix](crate::DecimalPrefix) impl of a [scale!] line with `prefix P`.
#[doc(hidden)]
#[macro_export]
macro_rules! __scale_decimal {
    ([$($attr:tt)*] $name:ident) => {};
    ([$($attr:tt)*] $name:ident $prefix:ident) => {
        $crate::__with_cfgs! {
            [] $($attr)* =>
            impl $crate::DecimalPrefix for $name {
                const PREFIX: $crate::Prefix = $crate::Prefix::$prefix;
            }
        }
    };
}

/// `Some(Prefix::P)` of a [scale!] line with `prefix P`.
#[doc(hidden)]
#[macro_export]
macro_rules! __scale_runtime {
    () => {
        None
    };
    ($prefix:ident) => {
        Some($crate::Prefix::$prefix)
    };
}

/// `num / den, pow10 a, pow2 b, pi c` as a [Magnitude](crate::Magnitude).
#[doc(hidden)]
#[macro_export]
macro_rules! __magnitude {
    ($num:literal / $den:literal $(, pow10 $p10:literal)? $(, pow2 $p2:literal)? $(, pi $pi:literal)? $(,)?) => {
        $crate::Magnitude::new(
            $num,
            $den,
            $crate::__or_zero!($($p10)?),
            $crate::__or_zero!($($p2)?),
            $crate::__or_zero!($($pi)?),
        )
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __or_zero {
    () => {
        0
    };
    ($e:literal) => {
        $e
    };
}

//! `ww_si` quantities in dynamic clients (CLT-31): a value of `ww_si::Quantity<U>` (ww_si 0.2 and later) known only
//! from introspection data, with its unit.
//!
//! A use of `Volt` in an API is `Quantity<units::Volt>`; once the bundle is
//! [prepared](ww_self::generics::prepare), it is a concrete definition that records the generic one and its arguments
//! ([instance_of](ww_self::ItemStructOwned::instance_of)). [unit_of] finds the unit marker there and looks it up:
//!
//! - a marker of `ww_si::units` by its name in [ww_si::units::by_name] (symbol, dimension and scale known),
//! - `units::Per<A, B>`, `units::Times<A, B>` and `units::Sqrt<A>` as a tree of their parts (`N·m/(rad/s)`),
//! - a unit of any other crate (a user's `unit! { pub struct Rpm = "rpm", .. }`) by the
//!   [definition](ww_self::ItemStructOwned::marker) the bundle carries (INT-13): symbol, dimension and exact factor,
//! - a marker without a definition (hand-written, from a bundle of before INT-13) by its name and docs only.
//!
//! [ww_self::units] resolves the same types to a canonical expression, for comparing units across crates.
//!
//! A value of `ww_si::AnyQuantity` (STD-37) carries its unit as an `SIExpr`: a named unit `ww_si` has becomes the same
//! [DynUnit] as the typed quantity's, a device's own named unit keeps the dimension and scale it sent.
//!
//! [DynQuantity] is a value with that unit: `3300 mV` as stored, `3.3 V` with `{:#}`, [DynQuantity::to_si_f64]. Units
//! are shown, named and converted by `ww_si::tree`, the code that does it for typed quantities.

use std::fmt;
use wire_weaver::shrink_wrap::{DeserializeShrinkWrap, RefBox, SerializeShrinkWrapOwned};
use ww_numeric::{NumericBaseType, NumericValue};
pub use ww_self::units::NumBounds;
use ww_self::units::{Wrapper, wrapper_of};
use ww_self::{
    ApiBundleOwned, FieldsOwned, FieldsValueOwned, GenericArgOwned, InstanceOfOwned,
    ItemStructOwned, MarkerDefOwned, TypeOwned, ValueOwned,
};
use ww_si::tree::{self, Tree};
use ww_si::{AnyQuantity, AnyQuantityOwned, Magnitude, SIExpr, SIExprOwned, Scale};
pub use ww_si::{Dim, DimOwned, MAX_UNIT_DEPTH, Prefix, UnitExpr};

/// The unit of a `Quantity<U>`, built from `U` as introspection data describes it, or of an `AnyQuantity`, built
/// from the `SIExpr` the device sent with the value.
// A few per API on a host, the size of a named one doesn't matter there
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum DynUnit {
    /// A named unit of `ww_si::units`: symbol, dimension and scale are known.
    Known(&'static UnitExpr),
    /// A named unit `ww_si` doesn't have (a device's own `Rpm`): symbol, dimension and scale as the device sent them
    /// in an `AnyQuantity`, or as the bundle defines the marker of a `Quantity<U>` (INT-13, an `Exact` scale).
    Named {
        name: String,
        symbol: String,
        dim: DimOwned,
        scale: Scale,
    },
    /// A marker without a definition in the bundle (hand-written, or from a bundle of before INT-13): its name and
    /// docs, nothing else.
    Unknown { name: String, docs: Vec<String> },
    /// `units::Per<A, B>`, `A / B`.
    Per(Box<DynUnit>, Box<DynUnit>),
    /// `units::Times<A, B>`, `A · B`.
    Times(Box<DynUnit>, Box<DynUnit>),
    /// `A` to an integer power (`UnitExpr::Pow`, `SIExpr::Pow`).
    Pow(Box<DynUnit>, i8),
    /// `units::Sqrt<A>`, `√A` (`UnitExpr::Sqrt`, `SIExpr::Sqrt`).
    Sqrt(Box<DynUnit>),
}

impl DynUnit {
    /// The unit of a typed `Quantity<U>`, `U::EXPR`: the same tree a dynamic client builds from introspection data.
    pub fn from_expr(expr: &'static UnitExpr) -> Self {
        match expr {
            UnitExpr::Named { .. } => DynUnit::Known(expr),
            UnitExpr::Mul(a, b) => {
                DynUnit::Times(Box::new(Self::from_expr(a)), Box::new(Self::from_expr(b)))
            }
            UnitExpr::Div(a, b) => {
                DynUnit::Per(Box::new(Self::from_expr(a)), Box::new(Self::from_expr(b)))
            }
            UnitExpr::Pow(a, n) => DynUnit::Pow(Box::new(Self::from_expr(a)), *n),
            UnitExpr::Sqrt(a) => DynUnit::Sqrt(Box::new(Self::from_expr(a))),
        }
    }

    /// The unit of an `AnyQuantity`: a named unit `ww_si` has is [Known](DynUnit::Known), so it is the same
    /// `DynUnit` as the typed quantity's. Followed [MAX_UNIT_DEPTH] levels deep, a part that is deeper or doesn't
    /// decode is an unknown `?`.
    pub fn from_si_expr(expr: &SIExpr<'_>) -> Self {
        fn conv(e: &SIExpr<'_>, depth: usize) -> DynUnit {
            let unknown = || DynUnit::Unknown {
                name: "?".into(),
                docs: vec![],
            };
            if depth >= MAX_UNIT_DEPTH {
                return unknown();
            }
            let child = |r: &RefBox<'_, SIExpr<'_>>| match r.read() {
                Ok(e) => Box::new(conv(&e, depth + 1)),
                Err(_) => Box::new(unknown()),
            };
            match e {
                SIExpr::Named {
                    name,
                    symbol,
                    dim,
                    scale,
                } => match e.known() {
                    Some(known) => DynUnit::Known(known),
                    None => DynUnit::Named {
                        name: name.to_string(),
                        symbol: symbol.to_string(),
                        dim: (*dim).into(),
                        scale: *scale,
                    },
                },
                SIExpr::Mul(a, b) => DynUnit::Times(child(a), child(b)),
                SIExpr::Div(a, b) => DynUnit::Per(child(a), child(b)),
                SIExpr::Pow(a, n) => DynUnit::Pow(child(a), *n),
                SIExpr::Sqrt(a) => DynUnit::Sqrt(child(a)),
            }
        }
        conv(expr, 0)
    }

    /// The unit as it goes on the wire in an `AnyQuantity`, `None` when a part of it is an unknown marker (no
    /// dimension or scale to send).
    pub fn to_si_expr(&self) -> Option<SIExprOwned> {
        Some(match self {
            DynUnit::Known(expr) => SIExprOwned::from(*expr),
            DynUnit::Named {
                name,
                symbol,
                dim,
                scale,
            } => SIExprOwned::Named {
                name: name.clone(),
                symbol: symbol.clone(),
                dim: dim.clone(),
                scale: *scale,
            },
            DynUnit::Unknown { .. } => return None,
            DynUnit::Per(a, b) => {
                SIExprOwned::Div(Box::new(a.to_si_expr()?), Box::new(b.to_si_expr()?))
            }
            DynUnit::Times(a, b) => {
                SIExprOwned::Mul(Box::new(a.to_si_expr()?), Box::new(b.to_si_expr()?))
            }
            DynUnit::Pow(a, n) => SIExprOwned::Pow(Box::new(a.to_si_expr()?), *n),
            DynUnit::Sqrt(a) => SIExprOwned::Sqrt(Box::new(a.to_si_expr()?)),
        })
    }

    /// The type name: `Volt`, `Per<Torque, AngularVelocity>`, `Rpm`.
    pub fn name(&self) -> String {
        tree::name(&Part::Dyn(self)).to_string()
    }

    /// The symbol as written after a number: `V`, `N·m/(rad/s)`, empty for a ratio; an unknown marker's name stands
    /// for its symbol. Same as `Display`.
    pub fn symbol(&self) -> String {
        self.to_string()
    }

    /// SI base-unit exponents, `None` when a part of the unit is an unknown marker.
    pub fn dim(&self) -> Option<Dim<'_>> {
        tree::dim(&Part::Dyn(self))
    }

    /// Factor to coherent SI (offsets left out, as in [UnitExpr::factor]), `None` with an unknown marker in it.
    pub fn factor(&self) -> Option<f64> {
        tree::factor(&Part::Dyn(self))
    }

    /// The exact factor to coherent SI, `None` with an unknown marker or a `Factor` / `Affine` scale in it: a
    /// pound-force second is 0.45359237 · 9.80665 N·s.
    pub fn magnitude(&self) -> Option<Magnitude> {
        tree::magnitude(&Part::Dyn(self))
    }

    /// A unit of `ww_si` by its type name, as [name](Self::name) writes it: `Newton`, `Times<PoundForce, Second>`,
    /// `Per<Meter, Second>`, `Pow<Meter, 3>`, `Sqrt<Hertz>`. Spaces don't matter.
    pub fn from_name(name: &str) -> Result<DynUnit, String> {
        let squeezed: String = name.chars().filter(|c| !c.is_whitespace()).collect();
        match parse_name(&squeezed, 0)? {
            (unit, "") => Ok(unit),
            (_, rest) => Err(format!("`{name}`: `{rest}` after the unit")),
        }
    }

    /// Offset to coherent SI: only a named unit with an affine scale (°C) has one.
    pub fn offset(&self) -> f64 {
        tree::offset(&Part::Dyn(self))
    }

    /// Docs of an unknown marker, empty otherwise.
    pub fn docs(&self) -> &[String] {
        match self {
            DynUnit::Unknown { docs, .. } => docs,
            _ => &[],
        }
    }
}

/// A [DynUnit] or a part of a `UnitExpr` in it, walked by `ww_si::tree`: shown, named and converted by the same
/// code as a typed quantity's unit.
#[derive(Clone, Copy)]
enum Part<'a> {
    Dyn(&'a DynUnit),
    Expr(&'static UnitExpr),
}

impl<'a> Tree<'a> for Part<'a> {
    fn node(&self) -> Option<tree::Node<'a, Self>> {
        match self {
            Part::Expr(expr) => Some(expr.node()?.map(Part::Expr)),
            Part::Dyn(unit) => Some(match unit {
                DynUnit::Known(expr) => expr.node()?.map(Part::Expr),
                DynUnit::Named {
                    name,
                    symbol,
                    dim,
                    scale,
                } => tree::Node::Named {
                    name,
                    symbol,
                    dim: Some(dim.as_dim()),
                    scale: Some(*scale),
                },
                DynUnit::Unknown { name, .. } => tree::Node::Named {
                    name,
                    symbol: name,
                    dim: None,
                    scale: None,
                },
                DynUnit::Per(a, b) => tree::Node::Div(Part::Dyn(a), Part::Dyn(b)),
                DynUnit::Times(a, b) => tree::Node::Mul(Part::Dyn(a), Part::Dyn(b)),
                DynUnit::Pow(a, n) => tree::Node::Pow(Part::Dyn(a), *n),
                DynUnit::Sqrt(a) => tree::Node::Sqrt(Part::Dyn(a)),
            }),
        }
    }
}

/// The symbol, written as `ww_si` writes the same unit: `V`, `N·m/(rad/s)`, `(rad/s)·(rad/s)`.
impl fmt::Display for DynUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&tree::symbol(&Part::Dyn(self)), f)
    }
}

/// The scale of a quantity's number: an SI prefix (`m` in `3300 mV`), or any other exact scale of an
/// `exact_units::Fixed` (CLT-54): a binary prefix (`Ki`), a user's `scale!`, a power of two without a symbol (`Q15`'s
/// 2⁻¹⁵).
// TODO(CLT-54): one `Prefix` once ww_si's has the binary prefixes and `Exact` (STD-59)
#[derive(Clone, Debug, PartialEq)]
pub enum DynPrefix {
    Si(Prefix),
    /// Written before the unit's symbol; empty for a factor shown as such (`3·2⁻¹⁵`).
    Exact {
        symbol: String,
        magnitude: Magnitude,
    },
}

/// The binary prefixes, which text may carry (`4 KiB`) wherever a number is converted exactly.
const BINARY: [(&str, i8); 4] = [("Ki", 10), ("Mi", 20), ("Gi", 30), ("Ti", 40)];

impl DynPrefix {
    pub const UNIT: DynPrefix = DynPrefix::Si(Prefix::Unit);

    /// The scale with `symbol` and `magnitude`: an SI prefix when it is a power of ten one has, as `Fixed` shows it.
    pub fn new(symbol: &str, magnitude: Magnitude) -> Self {
        match magnitude.as_pow10().and_then(|e| Prefix::try_from(e).ok()) {
            Some(p) => DynPrefix::Si(p),
            None => DynPrefix::Exact {
                symbol: symbol.into(),
                magnitude,
            },
        }
    }

    /// The exact factor: 10⁻³ for `m`, 2¹⁰ for `Ki`.
    pub fn magnitude(&self) -> Magnitude {
        match self {
            DynPrefix::Si(p) => Magnitude::pow10((*p).into()),
            DynPrefix::Exact { magnitude, .. } => *magnitude,
        }
    }

    /// The SI prefix, `None` for another scale.
    pub fn si(&self) -> Option<Prefix> {
        match self {
            DynPrefix::Si(p) => Some(*p),
            DynPrefix::Exact { .. } => None,
        }
    }

    /// Written before the unit: `m`, `µ`, `Ki`; empty for the unit itself and for a scale without a symbol.
    pub fn symbol(&self) -> &str {
        match self {
            DynPrefix::Si(p) => p.symbol(),
            DynPrefix::Exact { symbol, .. } => symbol,
        }
    }

    /// `Milli`, `Unit`; another scale by its symbol, or its factor (`2⁻¹⁵`) without one.
    pub fn name(&self) -> String {
        match self {
            DynPrefix::Si(p) => p.name().into(),
            DynPrefix::Exact { symbol, .. } if !symbol.is_empty() => symbol.clone(),
            DynPrefix::Exact { magnitude, .. } => {
                magnitude.to_string().trim_start_matches('·').into()
            }
        }
    }

    /// `x` times the factor.
    pub fn scale_f64(&self, x: f64) -> f64 {
        match self {
            DynPrefix::Si(p) => p.scale_f64(x),
            DynPrefix::Exact { magnitude, .. } => magnitude.apply_f64(x),
        }
    }
}

impl From<Prefix> for DynPrefix {
    fn from(p: Prefix) -> Self {
        DynPrefix::Si(p)
    }
}

/// What a `Fixed` stores its number in: the number type and the bounds marker's range.
#[derive(Clone, Debug, PartialEq)]
pub struct Storage {
    pub base: NumericBaseType,
    pub bounds: NumBounds,
}

impl Storage {
    /// `u32`, `ub4`, `f32`.
    pub fn name(&self) -> String {
        ww_self::expr::base_name(&self.base)
    }

    /// The number type is a float.
    pub fn is_float(&self) -> bool {
        matches!(self.base, NumericBaseType::F32 | NumericBaseType::F64)
    }
}

/// An `exact_units::Fixed<U, P, T, B>` as introspection data describes it: unit, scale and storage (CLT-54).
#[derive(Clone, Debug, PartialEq)]
pub struct FixedType {
    pub unit: DynUnit,
    pub prefix: DynPrefix,
    pub storage: Storage,
}

impl FixedType {
    /// What follows its numbers: `µs`, `KiB`, `·33/40960 V` for a scale without a symbol.
    pub fn symbol(&self) -> String {
        prefix_unit(&self.prefix, &self.unit)
    }
}

/// `ty` as a `Fixed`, `None` for any other type or one whose scale or storage doesn't resolve.
pub fn fixed_of(ty: &TypeOwned, bundle: &ApiBundleOwned) -> Option<FixedType> {
    let Wrapper::Fixed {
        unit: u,
        scale,
        storage,
        bounds,
    } = wrapper_of(ty, bundle)?
    else {
        return None;
    };
    let magnitude = ww_self::units::scale_of(scale, bundle).ok()?;
    let symbol = ww_self::units::scale_symbol_of(scale, bundle).ok()?;
    Some(FixedType {
        unit: unit(u, bundle, 0),
        prefix: DynPrefix::new(&symbol, magnitude),
        storage: Storage {
            base: ww_self::units::storage_of(storage, bundle).ok()?,
            bounds: ww_self::units::bounds_of(bounds, bundle).ok()?,
        },
    })
}

/// A value of a `ww_si::Quantity<U>`, a `ww_si::AnyQuantity` or an `exact_units::Fixed<U, P, T, B>`: the prefix and
/// number the device sent, and the unit.
#[derive(Clone, Debug, PartialEq)]
pub struct DynQuantity {
    pub prefix: DynPrefix,
    pub value: NumericValue,
    pub unit: DynUnit,
    /// `Some` for a `Fixed`: its number type and bounds. A `Quantity` stores any number.
    pub storage: Option<Storage>,
}

impl DynQuantity {
    /// A quantity of an SI prefix, as a `Quantity` holds it.
    pub fn new(prefix: Prefix, value: NumericValue, unit: DynUnit) -> Self {
        DynQuantity {
            prefix: prefix.into(),
            value,
            unit,
            storage: None,
        }
    }

    /// `value` of type `ty` as a quantity, `None` when `ty` is not a `ww_si::Quantity<U>`, `ww_si::AnyQuantity` or
    /// `exact_units::Fixed`, or the value doesn't match it.
    pub fn from_value(value: &ValueOwned, ty: &TypeOwned, bundle: &ApiBundleOwned) -> Option<Self> {
        if is_any_quantity(ty, bundle) {
            // decoded by the derived type, the unit tree walked with its depth bound
            let bytes = value.ser_shrink_wrap_dyn(ty, bundle).ok()?;
            let q = AnyQuantity::from_ww_bytes(&bytes).ok()?;
            return Some(DynQuantity::new(
                q.prefix,
                q.value,
                DynUnit::from_si_expr(&q.unit),
            ));
        }
        if let Some(fixed) = fixed_of(ty, bundle) {
            return Some(DynQuantity {
                prefix: fixed.prefix,
                value: wrapped_number(value)?,
                unit: fixed.unit,
                storage: Some(fixed.storage),
            });
        }
        let unit = unit_of(ty, bundle)?;
        let ValueOwned::Struct {
            fields: FieldsValueOwned::Named(fields),
        } = value
        else {
            return None;
        };
        let mut prefix = None;
        let mut number = None;
        for (name, value) in fields {
            match (name.as_str(), value) {
                (
                    "prefix",
                    ValueOwned::Enum {
                        variant,
                        fields: FieldsValueOwned::Unit,
                    },
                ) => prefix = Prefix::from_name(variant),
                ("value", ValueOwned::Enum { fields, .. }) => number = numeric_value(fields),
                _ => {}
            }
        }
        Some(DynQuantity::new(prefix?, number?, unit))
    }

    /// The value of `ty` (a `ww_si::Quantity` of the same unit, an `AnyQuantity` with this unit, or a `Fixed` of a unit
    /// it converts to exactly) this quantity is, for writing it. Into a `Fixed` the number is converted exactly into
    /// its scale and storage: `5 ms` into `Fixed<Second, Micro, u32>` is 5000; `1.5 µs`, a number past `u32` or
    /// outside the bounds is an error naming the type.
    pub fn to_value(&self, ty: &TypeOwned, bundle: &ApiBundleOwned) -> Result<ValueOwned, String> {
        if let Some(fixed) = fixed_of(ty, bundle) {
            let number = self.fixed_number(&fixed, &type_name(ty, bundle))?;
            return wrapper_value(number, ty, bundle);
        }
        let (prefix, value) = self.si_parts()?;
        if is_any_quantity(ty, bundle) {
            let Some(unit) = self.unit.to_si_expr() else {
                return Err(format!(
                    "a quantity in {} has no dimension and scale to send in an AnyQuantity",
                    self.unit.name()
                ));
            };
            let bytes = AnyQuantityOwned {
                prefix,
                value,
                unit,
            }
            .to_ww_bytes_owned()
            .map_err(|e| format!("{e:?}"))?;
            return ValueOwned::des_shrink_wrap_dyn(&bytes, ty, bundle)
                .map_err(|e| format!("{e:#}"));
        }
        let Some(unit) = unit_of(ty, bundle) else {
            return Err(format!(
                "a quantity in {} where {} is expected",
                self.unit.name(),
                type_name(ty, bundle)
            ));
        };
        if unit != self.unit {
            return Err(format!(
                "a quantity in {} where one in {} is expected",
                self.unit.name(),
                unit.name()
            ));
        }
        // the unit is only in the type (`PhantomData`): every `Quantity<U>` has the same bytes
        let bytes = ww_si::Quantity::<ww_si::units::Ratio>::from_parts(prefix, value)
            .to_ww_bytes_owned()
            .map_err(|e| format!("{e:?}"))?;
        ValueOwned::des_shrink_wrap_dyn(&bytes, ty, bundle).map_err(|e| format!("{e:#}"))
    }

    /// The SI prefix and number, as a `ww_si::Quantity` holds them: another scale (`Ki`) converts to one exactly, or
    /// is an error.
    pub fn si_parts(&self) -> Result<(Prefix, NumericValue), String> {
        match self.prefix.si() {
            Some(p) => Ok((p, self.value)),
            None => {
                let si = self.convert(&self.unit)?;
                Ok((si.prefix.si().unwrap_or(Prefix::Unit), si.value))
            }
        }
    }

    /// The number a `Fixed` of type `ty_name` stores for this quantity, exactly, or an error.
    fn fixed_number(&self, fixed: &FixedType, ty_name: &str) -> Result<NumericValue, String> {
        let refuse = |why: String| format!("{self} can't be written into {ty_name}: {why}");
        // stored = value · factor, factor = this scale / the stored one, through the units when they differ
        let mut factor = self
            .prefix
            .magnitude()
            .checked_div(fixed.prefix.magnitude());
        if self.unit != fixed.unit {
            let (Some(from), Some(to)) = (self.unit.dim(), fixed.unit.dim()) else {
                return Err(refuse(format!(
                    "{} is not {}",
                    self.unit.name(),
                    fixed.unit.name()
                )));
            };
            if from != to {
                return Err(refuse(format!("dimension {from} is not {to}")));
            }
            if self.unit.offset() != 0.0 || fixed.unit.offset() != 0.0 {
                return Err(refuse("a unit with an offset converts as a point".into()));
            }
            let (Some(from), Some(to)) = (self.unit.magnitude(), fixed.unit.magnitude()) else {
                return Err(refuse("a unit without an exact factor to SI".into()));
            };
            factor = factor
                .and_then(|f| f.checked_mul(from))
                .and_then(|f| f.checked_div(to));
        }
        let factor =
            factor.map_err(|_| refuse("the factor leaves what a Magnitude holds".into()))?;
        let number = if fixed.storage.is_float() {
            let x = factor.apply_f64(self.value.to_f64());
            match fixed.storage.base {
                NumericBaseType::F32 => NumericValue::F32(x as f32),
                _ => NumericValue::F64(x),
            }
        } else {
            // the number as written, a float by its shortest decimal text, so `0.1 ms` is exactly 100 µs
            let (mantissa, exp) =
                decimal_of(&self.value).ok_or_else(|| refuse("not a finite number".into()))?;
            let stored = Magnitude::checked_pow10(exp)
                .and_then(|p| p.checked_mul(factor).ok())
                .and_then(|f| f.apply_int(mantissa))
                .ok_or_else(|| {
                    refuse(format!(
                        "not a whole number of {}",
                        prefix_unit(&fixed.prefix, &fixed.unit)
                    ))
                })?;
            ww_self::expr::int_value(stored, &fixed.storage.base)
                .map_err(|e| refuse(e.to_string()))?
        };
        if !fixed.storage.bounds.contains(&number) {
            return Err(refuse(format!(
                "{} is out of {}",
                show_number(&number),
                fixed.storage.bounds
            )));
        }
        Ok(number)
    }

    /// The value in the unit itself (no prefix) as `f64`, as `ww_si::Quantity::to_f64`.
    pub fn to_f64(&self) -> f64 {
        self.prefix.scale_f64(self.value.to_f64())
    }

    /// This quantity in `unit`, exactly, or an error: never the number reinterpreted in another unit. The units have
    /// the same dimension and exact factors to SI (a pound-force second converts to newton seconds, a pound of mass
    /// doesn't, nor does a pound-force to a pound). An integer comes out as an integer with the largest SI prefix
    /// that keeps it whole (2 lbf·s is `8896443230521 p(N·s)`, 0 is `0` in the unit itself), an error when no prefix does (a factor that is not a
    /// fraction of integers whose denominator has only the primes 2 and 5, or a number past `i64`); a float is
    /// scaled and comes out as an `f64` in the unit itself. A unit with an offset (°C) converts as a point, not here.
    pub fn convert(&self, unit: &DynUnit) -> Result<DynQuantity, String> {
        let refuse = |why: &str| format!("{} can't be converted to {}: {why}", self, unit.symbol());
        let (Some(from), Some(to)) = (self.unit.dim(), unit.dim()) else {
            return Err(refuse("a unit known by its name only"));
        };
        if from != to {
            return Err(refuse(&format!("dimension {from} is not {to}")));
        }
        if self.unit.offset() != 0.0 || unit.offset() != 0.0 {
            return Err(refuse("a unit with an offset converts as a point"));
        }
        let (Some(from), Some(to)) = (self.unit.magnitude(), unit.magnitude()) else {
            return Err(refuse("a unit without an exact factor to SI"));
        };
        let factor = Some(self.prefix.magnitude())
            .and_then(|p| p.checked_mul(from).ok())
            .and_then(|m| m.checked_div(to).ok())
            .ok_or_else(|| refuse("the factor leaves what a Magnitude holds"))?;
        if self.value.is_float() {
            return Ok(DynQuantity::new(
                Prefix::Unit,
                NumericValue::F64(self.value.to_f64() * factor.to_f64()),
                unit.clone(),
            ));
        }
        let x = self.value.to_i128().ok_or_else(|| refuse("not a number"))?;
        if x == 0 {
            return Ok(DynQuantity::new(
                Prefix::Unit,
                NumericValue::I32(0),
                unit.clone(),
            ));
        }
        let (num, den) = factor
            .to_ratio()
            .ok_or_else(|| refuse("the factor has a power of π"))?;
        let exact = |e: i8| -> Option<i64> {
            // x · num / den / 10^e
            let p = Magnitude::checked_pow10(e)?.to_ratio()?;
            let (n, d) = (
                i128::try_from(num.checked_mul(p.1)?).ok()?,
                i128::try_from(den.checked_mul(p.0)?).ok()?,
            );
            let y = x.checked_mul(n)?;
            (y % d == 0).then(|| i64::try_from(y / d).ok()).flatten()
        };
        // the largest prefix that keeps it whole: the smallest number
        let (prefix, y) = Prefix::ALL
            .into_iter()
            .filter_map(|p| exact(p.into()).map(|y| (p, y)))
            .max_by_key(|(p, _)| i8::from(*p))
            .ok_or_else(|| {
                refuse(
                    "no SI prefix holds it as a whole number (convert a float for a rounded value)",
                )
            })?;
        let value = match i32::try_from(y) {
            Ok(y) => NumericValue::I32(y),
            Err(_) => NumericValue::I64(y),
        };
        Ok(DynQuantity::new(prefix, value, unit.clone()))
    }

    /// The value in coherent SI through the unit's scale (90 ° is 1.5708 rad, 20 °C is 293.15 K), `None` when the
    /// unit has an unknown marker in it.
    pub fn to_si_f64(&self) -> Option<f64> {
        Some(self.to_f64() * self.unit.factor()? + self.unit.offset())
    }
}

/// One unit at the start of `s` and what follows it, for [DynUnit::from_name].
fn parse_name(s: &str, depth: usize) -> Result<(DynUnit, &str), String> {
    if depth >= MAX_UNIT_DEPTH {
        return Err(format!("a unit deeper than {MAX_UNIT_DEPTH} levels"));
    }
    let end = s
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(s.len());
    let (ident, rest) = s.split_at(end);
    let Some(args) = rest.strip_prefix('<') else {
        let expr = ww_si::units::by_name(ident)
            .ok_or_else(|| format!("`{ident}` is not a named unit of ww_si"))?;
        return Ok((DynUnit::Known(expr), rest));
    };
    let (a, rest) = parse_name(args, depth + 1)?;
    fn close(rest: &str) -> Result<&str, String> {
        rest.strip_prefix('>')
            .ok_or_else(|| format!("`>` expected at `{rest}`"))
    }
    let a = Box::new(a);
    match ident {
        "Sqrt" => Ok((DynUnit::Sqrt(a), close(rest)?)),
        "Times" | "Per" | "Pow" => {
            let rest = rest
                .strip_prefix(',')
                .ok_or_else(|| format!("`,` expected at `{rest}`"))?;
            if ident == "Pow" {
                let end = rest.find('>').unwrap_or(rest.len());
                let n: i8 = rest[..end]
                    .parse()
                    .map_err(|_| format!("an exponent expected at `{rest}`"))?;
                return Ok((DynUnit::Pow(a, n), close(&rest[end..])?));
            }
            let (b, rest) = parse_name(rest, depth + 1)?;
            let b = Box::new(b);
            let unit = match ident {
                "Times" => DynUnit::Times(a, b),
                _ => DynUnit::Per(a, b),
            };
            Ok((unit, close(rest)?))
        }
        _ => Err(format!(
            "`{ident}<..>`: a compound unit is Times, Per, Pow or Sqrt"
        )),
    }
}

/// A quantity written as text, `3300 mV`, `3.3 V`, `-5 V`, `1.2e-4 N·m/(rad/s)`: a number, then the unit's symbol with
/// a prefix in front of it or none. Spaces don't matter, `*` stands for `·` and `u` for `µ`. `unit` is the unit the
/// value must be in (a `Quantity<U>`), the number alone is in the unit itself; `None` (an `AnyQuantity`) takes any
/// named unit of `ww_si` by its symbol. Integers are stored as `i32` (`i64` when larger), other numbers as `f32` when
/// that keeps the number as written, else `f64`.
pub fn parse_quantity(text: &str, unit: Option<&DynUnit>) -> Result<DynQuantity, String> {
    parse_quantity_in(text, unit, None)
}

/// [parse_quantity], where the text may also carry `scale`'s symbol in front of the unit (a `Fixed`'s own scale,
/// `Ki` or a user's `scale!`). Binary prefixes (`4 KiB`) are read too.
pub fn parse_quantity_in(
    text: &str,
    unit: Option<&DynUnit>,
    scale: Option<&DynPrefix>,
) -> Result<DynQuantity, String> {
    let squeezed: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let (number, symbol) = squeezed.split_at(number_len(&squeezed));
    let symbol = symbol.replace('*', "·");
    let value = number_value(number).ok_or_else(|| format!("`{text}`: expected a number first"))?;
    let (prefix, unit) = match unit {
        Some(unit) => {
            let prefix = prefix_before(&symbol, &unit.symbol(), scale).ok_or_else(|| {
                format!(
                    "`{text}`: expected a quantity in {} ({}), e.g. `1 {}` or with a prefix",
                    unit.name(),
                    unit.symbol(),
                    unit.symbol()
                )
            })?;
            (prefix, unit.clone())
        }
        None => ww_si::units::ALL
            .iter()
            .filter_map(|expr| match expr {
                UnitExpr::Named { symbol: s, .. } if !s.is_empty() => {
                    prefix_before(&symbol, s, None).map(|p| (p, DynUnit::Known(expr)))
                }
                _ => None,
            })
            // `mV` is milli volt, not a unit called `mV`: the unit without a prefix wins, then the longest symbol
            .max_by_key(|(p, u)| (*p == DynPrefix::UNIT, u.symbol().len()))
            .ok_or_else(|| format!("`{text}`: no unit of ww_si has the symbol `{symbol}`"))?,
    };
    Ok(DynQuantity {
        prefix,
        value,
        unit,
        storage: None,
    })
}

/// The prefix `symbol` has in front of `unit`'s symbol: `Unit` when it is that symbol (or empty). An SI or binary
/// prefix, or `scale`'s symbol; a compound unit's symbol may stand in parentheses after it (`k(N·m)`).
fn prefix_before(symbol: &str, unit: &str, scale: Option<&DynPrefix>) -> Option<DynPrefix> {
    let unit = unit.replace(' ', "");
    if symbol == unit || symbol.is_empty() {
        return Some(DynPrefix::UNIT);
    }
    let head = symbol
        .strip_suffix(unit.as_str())
        .or_else(|| symbol.strip_suffix(&format!("({unit})")))?;
    if head == "u" {
        return Some(Prefix::Micro.into());
    }
    if let Some(scale) = scale
        && !scale.symbol().is_empty()
        && scale.symbol() == head
    {
        return Some(scale.clone());
    }
    if let Some((s, e)) = BINARY.iter().find(|(s, _)| *s == head) {
        return Some(DynPrefix::Exact {
            symbol: (*s).into(),
            magnitude: Magnitude::pow2(*e),
        });
    }
    Prefix::ALL
        .into_iter()
        .find(|p| *p != Prefix::Unit && p.symbol() == head)
        .map(DynPrefix::Si)
}

/// Length of the number at the start of `text`: sign, digits, `_`, a point, an exponent when digits follow it (`5eV` is
/// 5 electron volt).
fn number_len(text: &str) -> usize {
    let b = text.as_bytes();
    let mut i = 0;
    if matches!(b.first(), Some(b'-' | b'+')) {
        i += 1;
    }
    while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'_' || b[i] == b'.') {
        i += 1;
    }
    if i < b.len() && matches!(b[i], b'e' | b'E') {
        let mut j = i + 1;
        if j < b.len() && matches!(b[j], b'-' | b'+') {
            j += 1;
        }
        if j < b.len() && b[j].is_ascii_digit() {
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            i = j;
        }
    }
    i
}

fn number_value(text: &str) -> Option<NumericValue> {
    let text = text.replace('_', "");
    if let Ok(i) = text.parse::<i64>() {
        return Some(match i32::try_from(i) {
            Ok(i) => NumericValue::I32(i),
            Err(_) => NumericValue::I64(i),
        });
    }
    let f: f64 = text.parse().ok()?;
    // f32, the storage every device has, when its shortest text is the same number
    Some(match (f as f32).to_string().parse::<f64>() == Ok(f) {
        true => NumericValue::F32(f as f32),
        false => NumericValue::F64(f),
    })
}

/// `3300 mV`, the number as stored with its prefix; `{:#}` writes it in the unit itself: `3.3 V`. The same text as
/// `ww_si::Quantity`'s `Display`, or `exact_units::Fixed`'s for a `Fixed` (`4 KiB`, `{:#}` `4096 B`, exactly), written
/// by the same code.
impl fmt::Display for DynQuantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let unit = Part::Dyn(&self.unit);
        if let (DynPrefix::Si(prefix), None) = (&self.prefix, &self.storage) {
            return fmt::Display::fmt(&tree::quantity(*prefix, &self.value, &unit), f);
        }
        let magnitude = self.prefix.magnitude();
        if f.alternate() {
            match self.value.to_i128() {
                Some(x) => match (magnitude.apply_int(x), magnitude.as_pow10()) {
                    (Some(x), _) => write!(f, "{x}")?,
                    (None, Some(e)) => write_decimal(x, e, f)?,
                    (None, None) => write!(f, "{}", magnitude.apply_f64(x as f64))?,
                },
                None => write!(f, "{}", magnitude.apply_f64(self.value.to_f64()))?,
            }
            return tree::write_unit(Prefix::Unit, &unit, f);
        }
        f.write_str(&show_number(&self.value))?;
        write_prefix_unit(&self.prefix, &self.unit, f)
    }
}

/// What follows a number: ` ms`, ` KiB`, `·2⁻¹⁵ V` for a factor without a symbol, as `exact_units::Fixed` writes it.
fn write_prefix_unit(
    prefix: &DynPrefix,
    unit: &DynUnit,
    f: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    match prefix {
        DynPrefix::Si(p) => tree::write_unit(*p, &Part::Dyn(unit), f),
        // a factor without a prefix: after the number, then the unit
        DynPrefix::Exact { symbol, magnitude } if symbol.is_empty() => {
            write!(f, "{magnitude}")?;
            tree::write_unit(Prefix::Unit, &Part::Dyn(unit), f)
        }
        DynPrefix::Exact { symbol, .. } => match unit.symbol().as_str() {
            "" => write!(f, " {symbol}"),
            u if matches!(unit, DynUnit::Known(_) | DynUnit::Named { .. }) => {
                write!(f, " {symbol}{u}")
            }
            u => write!(f, " {symbol}({u})"),
        },
    }
}

/// [write_prefix_unit] as text, without the space in front: `µs`, `KiB`.
fn prefix_unit(prefix: &DynPrefix, unit: &DynUnit) -> String {
    struct P<'a>(&'a DynPrefix, &'a DynUnit);
    impl fmt::Display for P<'_> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write_prefix_unit(self.0, self.1, f)
        }
    }
    P(prefix, unit).to_string().trim_start().to_string()
}

/// An integer at a power of ten below the unit, exactly: 1500 at 10⁻⁶ is `0.0015` (as `exact_units::Fixed` writes it).
fn write_decimal(x: i128, e: i8, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    if !(-38..0).contains(&e) {
        return write!(f, "{}", Magnitude::pow10(e).apply_f64(x as f64));
    }
    let digits = e.unsigned_abs() as u32;
    let p = 10u128.pow(digits);
    let m = x.unsigned_abs();
    let (int, mut frac) = (m / p, m % p);
    let mut width = digits as usize;
    while frac % 10 == 0 {
        frac /= 10;
        width -= 1;
    }
    let sign = if x < 0 { "-" } else { "" };
    write!(f, "{sign}{int}.{frac:0width$}")
}

/// The number as stored: integers exactly, floats by their shortest text (`f32` 3.3, not 3.299999952316284).
fn show_number(n: &NumericValue) -> String {
    match *n {
        NumericValue::F32(x) => x.to_string(),
        NumericValue::F64(x) => x.to_string(),
        n => match n.to_i128() {
            Some(i) => i.to_string(),
            None => n.to_f64().to_string(),
        },
    }
}

/// A number as mantissa and power of ten, exactly as written: an integer as is, a float by its shortest decimal
/// text (`0.1f32` is 1·10⁻¹). `None` for NaN and infinities.
fn decimal_of(n: &NumericValue) -> Option<(i128, i8)> {
    if let Some(i) = n.to_i128() {
        return Some((i, 0));
    }
    let text = show_number(n);
    let (int, frac) = text.split_once('.').unwrap_or((&text, ""));
    let mantissa: i128 = format!("{int}{frac}").parse().ok()?;
    Some((mantissa, -i8::try_from(frac.len()).ok()?))
}

/// The number of a transparent wrapper's value (`Fixed`, `Index`, ..): its one field.
fn wrapped_number(value: &ValueOwned) -> Option<NumericValue> {
    let field = match value {
        ValueOwned::Struct {
            fields: FieldsValueOwned::Named(f),
        } => f.first().map(|(_, v)| v),
        ValueOwned::Struct {
            fields: FieldsValueOwned::Unnamed(f),
        } => f.first(),
        _ => None,
    };
    match field? {
        ValueOwned::Numeric(n) => Some(*n),
        _ => None,
    }
}

/// The value of the transparent wrapper `ty` holding `number`, checked as the device checks it (bounds, an
/// `IndexFrom1`'s 0).
fn wrapper_value(
    number: NumericValue,
    ty: &TypeOwned,
    bundle: &ApiBundleOwned,
) -> Result<ValueOwned, String> {
    let Ok(TypeOwned::Struct(s)) = ty.get_in_line(bundle) else {
        return Err(format!("{} is not a struct", type_name(ty, bundle)));
    };
    let fields = match &s.fields {
        FieldsOwned::Named(f) => FieldsValueOwned::Named(vec![(
            f.first().and_then(|f| f.ident.clone()).unwrap_or_default(),
            ValueOwned::Numeric(number),
        )]),
        FieldsOwned::Unnamed(_) => FieldsValueOwned::Unnamed(vec![ValueOwned::Numeric(number)]),
        FieldsOwned::Unit => return Err(format!("{} holds no number", type_name(ty, bundle))),
    };
    ww_self::units::check_wrapper(s, &fields, bundle).map_err(|e| format!("{e:#}"))?;
    Ok(ValueOwned::Struct { fields })
}

/// A number as written (`5000`, `-3`, `1.5`) as the stored value of the `exact_units` wrapper `ty` (`Fixed`, `Index`,
/// `IndexFrom1`, `Seq`): the number itself, not in the unit (what a script wrote into the plain field before), range
/// and bounds checked. `None` when `ty` is no such wrapper.
pub fn stored_number(
    text: &str,
    ty: &TypeOwned,
    bundle: &ApiBundleOwned,
) -> Option<Result<ValueOwned, String>> {
    let wrapper = wrapper_of(ty, bundle)?;
    let storage = match wrapper {
        Wrapper::Fixed { storage, .. }
        | Wrapper::Index { storage, .. }
        | Wrapper::IndexFrom1 { storage, .. }
        | Wrapper::Seq { storage, .. } => storage,
    };
    let name = type_name(ty, bundle);
    Some((|| {
        let base = ww_self::units::storage_of(storage, bundle).map_err(|e| format!("{e:#}"))?;
        let text: String = text
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '_')
            .collect();
        let number = match base {
            NumericBaseType::F32 | NumericBaseType::F64 => {
                let x: f64 = text
                    .parse()
                    .map_err(|_| format!("`{text}` is not a number for {name}"))?;
                match base {
                    NumericBaseType::F32 => NumericValue::F32(x as f32),
                    _ => NumericValue::F64(x),
                }
            }
            _ => {
                let i: i128 = text
                    .parse()
                    .map_err(|_| format!("`{text}` is not a whole number, as {name} stores"))?;
                ww_self::expr::int_value(i, &base).map_err(|e| format!("{name}: {e:#}"))?
            }
        };
        wrapper_value(number, ty, bundle)
    })())
}

/// `ty`'s name for messages: `Fixed<Second, Micro, u32, Unbounded>`.
fn type_name(ty: &TypeOwned, bundle: &ApiBundleOwned) -> String {
    ty.human_name(false, bundle)
        .unwrap_or_else(|_| "another type".into())
}

/// What an `exact_units` `Index`, `IndexFrom1` or `Seq` counts with (CLT-54).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CountKind {
    /// 0-based position in a table.
    Index,
    /// 1-based, never 0.
    IndexFrom1,
    /// A wrapping sequence number.
    Seq,
}

/// A value of an `exact_units` `Index<Of, T, B>`, `IndexFrom1<Of, T, B>` or `Seq<Of, T>`: the number and what it
/// counts. Shown `#3 (Sample)`, `#1 (Channel, from 1)`, `seq 41 (Frame)`.
#[derive(Clone, Debug, PartialEq)]
pub struct DynCount {
    pub kind: CountKind,
    pub value: NumericValue,
    /// The `Of` marker: what is counted.
    pub of: DynUnit,
}

impl DynCount {
    /// `value` of type `ty` as a count, `None` when `ty` is no `Index`, `IndexFrom1` or `Seq`.
    pub fn from_value(value: &ValueOwned, ty: &TypeOwned, bundle: &ApiBundleOwned) -> Option<Self> {
        let (kind, of) = match wrapper_of(ty, bundle)? {
            Wrapper::Index { of, .. } => (CountKind::Index, of),
            Wrapper::IndexFrom1 { of, .. } => (CountKind::IndexFrom1, of),
            Wrapper::Seq { of, .. } => (CountKind::Seq, of),
            Wrapper::Fixed { .. } => return None,
        };
        Some(DynCount {
            kind,
            value: wrapped_number(value)?,
            of: unit(of, bundle, 0),
        })
    }
}

impl fmt::Display for DynCount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (n, of) = (show_number(&self.value), self.of.name());
        match self.kind {
            CountKind::Index => write!(f, "#{n} ({of})"),
            CountKind::IndexFrom1 => write!(f, "#{n} ({of}, from 1)"),
            CountKind::Seq => write!(f, "seq {n} ({of})"),
        }
    }
}

/// A `NumericValue` as the dynamic decoder gives it: an enum value, the variant named after the number type, the
/// number in it.
fn numeric_value(fields: &FieldsValueOwned) -> Option<NumericValue> {
    match fields {
        FieldsValueOwned::Unnamed(items) => match items.as_slice() {
            [ValueOwned::Numeric(n)] => Some(*n),
            _ => None,
        },
        _ => None,
    }
}

/// The unit of `ty` if it is a `ww_si::Quantity<U>` of ww_si 0.2 or later, in a [prepared](ww_self::generics::prepare)
/// bundle.
pub fn unit_of(ty: &TypeOwned, bundle: &ApiBundleOwned) -> Option<DynUnit> {
    let inst = instance_of(ty, bundle)?;
    if !is_ww_si(inst.type_idx.0, None, "Quantity", bundle) {
        return None;
    }
    let [GenericArgOwned::Type(u)] = inst.args.as_slice() else {
        return None;
    };
    Some(unit(u, bundle, 0))
}

/// Whether `ty` is a `ww_si::Quantity<U>`, see [unit_of], a `ww_si::AnyQuantity` or an `exact_units::Fixed`.
pub fn is_quantity(ty: &TypeOwned, bundle: &ApiBundleOwned) -> bool {
    unit_of(ty, bundle).is_some() || is_any_quantity(ty, bundle) || fixed_of(ty, bundle).is_some()
}

/// Whether `ty` is a `ww_si::AnyQuantity`, a value with its unit (ww_si 0.2 and later).
pub fn is_any_quantity(ty: &TypeOwned, bundle: &ApiBundleOwned) -> bool {
    matches!(ty.get_in_line(bundle), Ok(TypeOwned::Struct(s)) if is_ww_si_struct(s, None, "AnyQuantity", bundle))
}

fn instance_of<'a>(ty: &'a TypeOwned, bundle: &'a ApiBundleOwned) -> Option<&'a InstanceOfOwned> {
    match ty.get_in_line(bundle).ok()? {
        TypeOwned::Struct(s) => s.instance_of.as_ref(),
        TypeOwned::Enum(e) => e.instance_of.as_ref(),
        _ => None,
    }
}

/// `bundle.types[type_idx]` is `ww_si::<path>::<ident>` of ww_si 0.2 or later, or `exact_units::<path>::<ident>`.
fn is_ww_si(type_idx: u32, path: Option<&[&str]>, ident: &str, bundle: &ApiBundleOwned) -> bool {
    let Ok((TypeOwned::Struct(s), _)) = bundle.get_ty(type_idx) else {
        return false;
    };
    is_ww_si_struct(s, path, ident, bundle)
}

/// `s` is `ww_si::<path>::<ident>` of ww_si 0.2 or later, or `exact_units::<path>::<ident>`: the unit markers
/// moved there (STD-61), a device built against an older ww_si still names them in ww_si.
fn is_ww_si_struct(
    s: &ItemStructOwned,
    path: Option<&[&str]>,
    ident: &str,
    bundle: &ApiBundleOwned,
) -> bool {
    let Ok(version) = bundle.crate_version(s.crate_idx.0) else {
        return false;
    };
    let v = &version.version;
    let path_matches = match (&s.path, path) {
        (None, None) => true,
        (Some(a), Some(b)) => a.iter().map(String::as_str).eq(b.iter().copied()),
        _ => false,
    };
    let known_crate = match version.crate_id.as_str() {
        "ww_si" => (v.major.0, v.minor.0) >= (0, 2),
        "exact_units" => true,
        _ => false,
    };
    known_crate && s.ident == ident && path_matches
}

fn unit(ty: &TypeOwned, bundle: &ApiBundleOwned, depth: usize) -> DynUnit {
    let by_type_name = || DynUnit::Unknown {
        name: ty.human_name(false, bundle).unwrap_or_else(|_| "?".into()),
        docs: vec![],
    };
    if depth >= MAX_UNIT_DEPTH {
        return by_type_name();
    }
    let Ok(TypeOwned::Struct(s)) = ty.get_in_line(bundle) else {
        return by_type_name();
    };
    if let Some(inst) = &s.instance_of {
        let units = Some(&["units"][..]);
        // what the generic marker is: its definition (INT-13), or its name in a bundle of ww_si 0.2 without one
        let is = |def: MarkerDefOwned, name: &str| {
            matches!(bundle.get_ty(inst.type_idx.0), Ok((TypeOwned::Struct(g), _))
                if g.marker() == Some(&def))
                || is_ww_si(inst.type_idx.0, units, name, bundle)
        };
        if let [GenericArgOwned::Type(a), GenericArgOwned::Type(b)] = inst.args.as_slice() {
            let (a, b) = (
                Box::new(unit(a, bundle, depth + 1)),
                Box::new(unit(b, bundle, depth + 1)),
            );
            if is(MarkerDefOwned::Per, "Per") {
                return DynUnit::Per(a, b);
            }
            if is(MarkerDefOwned::Times, "Times") {
                return DynUnit::Times(a, b);
            }
        }
        if let [GenericArgOwned::Type(a)] = inst.args.as_slice()
            && is(MarkerDefOwned::Sqrt, "Sqrt")
        {
            return DynUnit::Sqrt(Box::new(unit(a, bundle, depth + 1)));
        }
        return by_type_name();
    }
    // a unit of ww_si / exact_units is the same `UnitExpr` a typed quantity has (its definition in the model is the
    // same, a test of wire_weaver_core checks), so both compare equal
    if let TypeOwned::OutOfLine { type_idx } = ty
        && is_ww_si(type_idx.0, Some(&["units"]), &s.ident, bundle)
        && let Some(expr) = ww_si::units::by_name(&s.ident)
    {
        return DynUnit::Known(expr);
    }
    // any other crate's unit by the definition the bundle carries
    if let Some(MarkerDefOwned::Unit(def)) = s.marker() {
        return DynUnit::Named {
            name: s.ident.clone(),
            symbol: def.symbol.clone(),
            dim: def.dim.clone(),
            scale: Scale::Exact(def.magnitude),
        };
    }
    // a marker without a definition: hand-written, from a bundle older than INT-13, or with an offset (TODO(STD-74))
    DynUnit::Unknown {
        name: s.ident.clone(),
        docs: s.docs.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wire_weaver::shrink_wrap::SerializeShrinkWrap;
    use ww_si::units::{
        AngularVelocity, Degree, DegreeCelsius, Meter, Per, Ratio, Second, Speed, Times, Torque,
    };

    #[test]
    fn quantities_from_text() {
        let any = |text: &str| {
            let q = parse_quantity(text, None).unwrap();
            (q.prefix.si().unwrap(), q.value, q.unit.symbol())
        };
        assert_eq!(
            any("20 °C"),
            (Prefix::Unit, NumericValue::I32(20), "°C".into())
        );
        // an exponent only with digits after it: 5 electron volt
        assert_eq!(
            any("5eV"),
            (Prefix::Unit, NumericValue::I32(5), "eV".into())
        );
        assert_eq!(
            any("1e3 eV"),
            (Prefix::Unit, NumericValue::F32(1000.0), "eV".into())
        );
        assert_eq!(
            any("4.7 kΩ"),
            (Prefix::Kilo, NumericValue::F32(4.7), "Ω".into())
        );
        assert_eq!(any("10 µs").0, Prefix::Micro);
        assert_eq!(any("10 us").0, Prefix::Micro);
        // a number f32 can't keep as written stays f64
        assert_eq!(
            any("3.141592653589793 m").1,
            NumericValue::F64(std::f64::consts::PI)
        );
        assert_eq!(any("3_000_000_000 m").1, NumericValue::I64(3_000_000_000));
        assert!(parse_quantity("20 furlongs", None).is_err());
        assert!(parse_quantity("V", None).is_err());
    }
    use ww_si::{Quantity, Unit};

    /// `Display` (both forms), `to_f64`, `to_si_f64` and the dimension of a typed quantity and of the dynamic one
    /// built from its parts.
    fn same<U: Unit>(q: Quantity<U>) {
        let d = DynQuantity::new(q.prefix, q.value, DynUnit::from_expr(U::EXPR));
        assert_eq!(d.to_string(), q.to_string());
        assert_eq!(format!("{d:#}"), format!("{q:#}"));
        assert_eq!(d.unit.symbol(), U::EXPR.symbol().to_string());
        assert_eq!(d.unit.name(), U::EXPR.name().to_string());
        assert_eq!(d.unit.dim(), Some(U::DIM));
        assert_eq!(d.to_f64(), q.to_f64());
        assert_eq!(d.to_si_f64(), Some(q.to_si_f64()));
        // as an AnyQuantity: its unit on the wire gives the same DynUnit back, and goes out the same way
        let any = ww_si::AnyQuantity::from(q);
        let mut buf = [0u8; 256];
        let bytes = any.to_ww_bytes(&mut buf).unwrap();
        let back = ww_si::AnyQuantity::from_ww_bytes(bytes).unwrap();
        assert_eq!(DynUnit::from_si_expr(&back.unit), d.unit);
        assert_eq!(d.unit.to_si_expr().unwrap(), SIExprOwned::from(U::EXPR));
    }

    #[test]
    fn written_as_typed_quantities_are() {
        same(Quantity::<Speed>::milli(1500u16));
        same(Quantity::<Ratio>::centi(42u8));
        same(Quantity::<Ratio>::micro(-3i32));
        same(Quantity::<Ratio>::kilo(1u8));
        same(Quantity::<Degree>::from_f32(90.0));
        same(Quantity::<DegreeCelsius>::base(20u8));
        same(Quantity::<Per<Torque, AngularVelocity>>::from_f32(1.2e-4));
        same(Quantity::<
            Per<Torque, Times<AngularVelocity, AngularVelocity>>,
        >::milli(5u8));
        same(Quantity::<Per<Per<Meter, Second>, Second>>::base(3u8));
        same(Quantity::<Times<Per<Meter, Second>, Second>>::micro(7u32));
        same(Quantity::<Times<Times<Meter, Meter>, Meter>>::kilo(2u8));
        same(Quantity::<Square>::base(4u8));
        same(Quantity::<Per<Meter, Square>>::milli(4u8));
    }

    /// A user's marker that is a power: `m²`.
    #[derive(Copy, Clone, Debug, Default)]
    struct Square;

    impl Unit for Square {
        const DIM: ww_si::Dim<'static> = ww_si::Dim::new(0, 2, 0, 0, 0, 0, 0);
        const EXPR: &'static UnitExpr = &UnitExpr::Pow(<Meter as Unit>::EXPR, 2);
        const SI_EXPR: &'static SIExpr<'static> = &SIExpr::Pow(
            RefBox::Ref {
                value: <Meter as Unit>::SI_EXPR,
            },
            2,
        );
    }

    #[test]
    fn powers_and_unknown_parts() {
        let d = DynUnit::from_expr(<Square as Unit>::EXPR);
        assert_eq!(d.to_string(), "m²");
        assert_eq!(d.name(), "Pow<Meter, 2>");
        let unknown = DynUnit::Pow(
            Box::new(DynUnit::Unknown {
                name: "Rpm".into(),
                docs: vec![],
            }),
            2,
        );
        assert_eq!(unknown.to_string(), "Rpm²");
        assert_eq!((unknown.dim(), unknown.factor()), (None, None));
        assert_eq!(unknown.to_si_expr(), None);
    }
}

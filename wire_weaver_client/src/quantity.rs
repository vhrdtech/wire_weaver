//! `ww_si` quantities in dynamic clients (CLT-31): a value of `ww_si::Quantity<U>` (ww_si 0.2 and later) known only
//! from introspection data, with its unit.
//!
//! A use of `Volt` in an API is `Quantity<units::Volt>`; once the bundle is
//! [prepared](ww_self::generics::prepare), it is a concrete definition that records the generic one and its arguments
//! ([instance_of](ww_self::ItemStructOwned::instance_of)). [unit_of] finds the unit marker there and looks it up:
//!
//! - a marker of `ww_si::units` by its name in [ww_si::units::by_name] (symbol, dimension and scale known),
//! - `units::Per<A, B>` and `units::Times<A, B>` as a tree of their parts (`N·m/(rad/s)`),
//! - any other marker (a user's own `struct Rpm;`) by its name and docs only.
//!
//! [DynQuantity] is a value with that unit: `3300 mV` as stored, `3.3 V` with `{:#}`, [DynQuantity::to_si_f64].

use std::fmt;
use wire_weaver::shrink_wrap::SerializeShrinkWrapOwned;
use ww_numeric::NumericValue;
use ww_self::{
    ApiBundleOwned, FieldsValueOwned, GenericArgOwned, InstanceOfOwned, TypeOwned, ValueOwned,
};
pub use ww_si::{Dim, Prefix, UnitExpr};

/// Deepest nesting of `Per` / `Times` followed in a unit, a deeper part is shown by its type name.
pub const MAX_UNIT_DEPTH: usize = 32;

/// The unit of a `Quantity<U>`, built from `U` as introspection data describes it.
#[derive(Clone, Debug, PartialEq)]
pub enum DynUnit {
    /// A named unit of `ww_si::units`: symbol, dimension and scale are known.
    Known(&'static UnitExpr),
    /// A marker `ww_si` doesn't know, a user's own (`struct Rpm;`): its name and docs, nothing else.
    Unknown { name: String, docs: Vec<String> },
    /// `units::Per<A, B>`, `A / B`.
    Per(Box<DynUnit>, Box<DynUnit>),
    /// `units::Times<A, B>`, `A · B`.
    Times(Box<DynUnit>, Box<DynUnit>),
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
        }
    }

    /// The type name: `Volt`, `Per<Torque, AngularVelocity>`, `Rpm`.
    pub fn name(&self) -> String {
        match self {
            DynUnit::Known(expr) => expr.name().to_string(),
            DynUnit::Unknown { name, .. } => name.clone(),
            DynUnit::Per(a, b) => format!("Per<{}, {}>", a.name(), b.name()),
            DynUnit::Times(a, b) => format!("Times<{}, {}>", a.name(), b.name()),
        }
    }

    /// The symbol as written after a number: `V`, `N·m/(rad/s)`, empty for a ratio; an unknown marker's name stands
    /// for its symbol. Same as `Display`.
    pub fn symbol(&self) -> String {
        self.to_string()
    }

    /// SI base-unit exponents, `None` when a part of the unit is an unknown marker.
    pub fn dim(&self) -> Option<Dim> {
        match self {
            DynUnit::Known(expr) => Some(expr.dim()),
            DynUnit::Unknown { .. } => None,
            DynUnit::Per(a, b) => Some(a.dim()?.div(b.dim()?)),
            DynUnit::Times(a, b) => Some(a.dim()?.mul(b.dim()?)),
        }
    }

    /// Factor to coherent SI (offsets left out, as in [UnitExpr::factor]), `None` with an unknown marker in it.
    pub fn factor(&self) -> Option<f64> {
        match self {
            DynUnit::Known(expr) => Some(expr.factor()),
            DynUnit::Unknown { .. } => None,
            DynUnit::Per(a, b) => Some(a.factor()? / b.factor()?),
            DynUnit::Times(a, b) => Some(a.factor()? * b.factor()?),
        }
    }

    /// Offset to coherent SI: only a named unit with an affine scale (°C) has one.
    pub fn offset(&self) -> f64 {
        match self {
            DynUnit::Known(expr) => expr.offset(),
            _ => 0.0,
        }
    }

    /// Docs of an unknown marker, empty otherwise.
    pub fn docs(&self) -> &[String] {
        match self {
            DynUnit::Unknown { docs, .. } => docs,
            _ => &[],
        }
    }

    /// Dimensionless with no symbol ([Ratio](ww_si::units::Ratio)): a prefix is written as %, ‰, ppm.
    fn is_ratio(&self) -> bool {
        matches!(self, DynUnit::Known(UnitExpr::Named { symbol: "", .. }))
    }

    fn is_named(&self) -> bool {
        matches!(
            self,
            DynUnit::Known(UnitExpr::Named { .. }) | DynUnit::Unknown { .. }
        )
    }

    /// Needs parentheses next to `·` or `/`, as [UnitExpr] writes it.
    fn is_compound(&self) -> bool {
        match self {
            DynUnit::Known(UnitExpr::Named { symbol, .. }) => symbol.contains(['/', '·']),
            DynUnit::Unknown { .. } => false,
            _ => true,
        }
    }
}

/// The symbol, written as `ww_si` writes the same unit: `V`, `N·m/(rad/s)`, `(rad/s)·(rad/s)`.
impl fmt::Display for DynUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn side(f: &mut fmt::Formatter<'_>, u: &DynUnit, parens: bool) -> fmt::Result {
            if parens {
                write!(f, "({u})")
            } else {
                write!(f, "{u}")
            }
        }
        match self {
            DynUnit::Known(expr) => write!(f, "{expr}"),
            DynUnit::Unknown { name, .. } => f.write_str(name),
            DynUnit::Times(a, b) => {
                side(f, a, a.is_compound() && !matches!(**a, DynUnit::Times(..)))?;
                f.write_str("·")?;
                side(f, b, b.is_compound() && !matches!(**b, DynUnit::Times(..)))
            }
            DynUnit::Per(a, b) => {
                side(f, a, matches!(**a, DynUnit::Per(..)))?;
                f.write_str("/")?;
                side(f, b, b.is_compound())
            }
        }
    }
}

/// A value of a `ww_si::Quantity<U>`: the prefix and number the device sent, and the unit.
#[derive(Clone, Debug, PartialEq)]
pub struct DynQuantity {
    pub prefix: Prefix,
    pub value: NumericValue,
    pub unit: DynUnit,
}

impl DynQuantity {
    /// `value` of type `ty` as a quantity, `None` when `ty` is not a `ww_si::Quantity<U>` or the value doesn't match
    /// it.
    pub fn from_value(value: &ValueOwned, ty: &TypeOwned, bundle: &ApiBundleOwned) -> Option<Self> {
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
        Some(DynQuantity {
            prefix: prefix?,
            value: number?,
            unit,
        })
    }

    /// The value of `ty` (a `ww_si::Quantity` of the same unit) this quantity is, for writing it.
    pub fn to_value(&self, ty: &TypeOwned, bundle: &ApiBundleOwned) -> Result<ValueOwned, String> {
        let Some(unit) = unit_of(ty, bundle) else {
            return Err(format!(
                "a quantity in {} where {} is expected",
                self.unit.name(),
                ty.human_name(false, bundle)
                    .unwrap_or_else(|_| "another type".into())
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
        let bytes = ww_si::Quantity::<ww_si::units::Ratio>::from_parts(self.prefix, self.value)
            .to_ww_bytes_owned()
            .map_err(|e| format!("{e:?}"))?;
        ValueOwned::des_shrink_wrap_dyn(&bytes, ty, bundle).map_err(|e| format!("{e:#}"))
    }

    /// The value in the unit itself (no prefix) as `f64`, as `ww_si::Quantity::to_f64`.
    pub fn to_f64(&self) -> f64 {
        self.prefix.scale_f64(self.value.to_f64())
    }

    /// The value in coherent SI through the unit's scale (90 ° is 1.5708 rad, 20 °C is 293.15 K), `None` when the
    /// unit has an unknown marker in it.
    pub fn to_si_f64(&self) -> Option<f64> {
        Some(self.to_f64() * self.unit.factor()? + self.unit.offset())
    }
}

/// `3300 mV`, the number as stored with its prefix; `{:#}` writes it in the unit itself: `3.3 V`. The same text as
/// `ww_si::Quantity`'s `Display`.
impl fmt::Display for DynQuantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            match self.value {
                NumericValue::F32(x) => write!(f, "{}", self.prefix.scale_f32(x))?,
                _ => write!(f, "{}", self.to_f64())?,
            }
            fmt_unit(Prefix::Unit, &self.unit, f)
        } else {
            fmt_number(&self.value, f)?;
            fmt_unit(self.prefix, &self.unit, f)
        }
    }
}

/// The number as stored: integers exactly, `f32` as `f32`.
fn fmt_number(value: &NumericValue, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match *value {
        NumericValue::F32(x) => write!(f, "{x}"),
        NumericValue::F64(x) => write!(f, "{x}"),
        value => match value.to_i128() {
            Some(x) => write!(f, "{x}"),
            None => write!(f, "{}", value.to_f64()),
        },
    }
}

/// The prefix and the unit's symbol after the number: ` mV`, ` %` for a ratio in `Centi`.
fn fmt_unit(prefix: Prefix, unit: &DynUnit, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    if unit.is_ratio() {
        return match prefix {
            Prefix::Unit => Ok(()),
            Prefix::Centi => f.write_str(" %"),
            Prefix::Milli => f.write_str(" ‰"),
            Prefix::Micro => f.write_str(" ppm"),
            p => write!(f, "e{}", i8::from(p)),
        };
    }
    match (prefix, unit.is_named()) {
        (Prefix::Unit, _) => write!(f, " {unit}"),
        (p, true) => write!(f, " {}{unit}", p.symbol()),
        (p, false) => write!(f, " {}({unit})", p.symbol()),
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

/// Whether `ty` is a `ww_si::Quantity<U>`, see [unit_of].
pub fn is_quantity(ty: &TypeOwned, bundle: &ApiBundleOwned) -> bool {
    unit_of(ty, bundle).is_some()
}

fn instance_of<'a>(ty: &'a TypeOwned, bundle: &'a ApiBundleOwned) -> Option<&'a InstanceOfOwned> {
    match ty.get_in_line(bundle).ok()? {
        TypeOwned::Struct(s) => s.instance_of.as_ref(),
        TypeOwned::Enum(e) => e.instance_of.as_ref(),
        _ => None,
    }
}

/// `bundle.types[type_idx]` is `ww_si::<path>::<ident>` of ww_si 0.2 or later.
fn is_ww_si(type_idx: u32, path: Option<&[&str]>, ident: &str, bundle: &ApiBundleOwned) -> bool {
    let Ok((TypeOwned::Struct(s), crate_idx)) = bundle.get_ty(type_idx) else {
        return false;
    };
    let Ok(version) = bundle.crate_version(crate_idx) else {
        return false;
    };
    let v = &version.version;
    let path_matches = match (&s.path, path) {
        (None, None) => true,
        (Some(a), Some(b)) => a.iter().map(String::as_str).eq(b.iter().copied()),
        _ => false,
    };
    version.crate_id == "ww_si"
        && (v.major.0, v.minor.0) >= (0, 2)
        && s.ident == ident
        && path_matches
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
        if let [GenericArgOwned::Type(a), GenericArgOwned::Type(b)] = inst.args.as_slice() {
            let (a, b) = (
                Box::new(unit(a, bundle, depth + 1)),
                Box::new(unit(b, bundle, depth + 1)),
            );
            if is_ww_si(inst.type_idx.0, units, "Per", bundle) {
                return DynUnit::Per(a, b);
            }
            if is_ww_si(inst.type_idx.0, units, "Times", bundle) {
                return DynUnit::Times(a, b);
            }
        }
        return by_type_name();
    }
    if let TypeOwned::OutOfLine { type_idx } = ty
        && is_ww_si(type_idx.0, Some(&["units"]), &s.ident, bundle)
        && let Some(expr) = ww_si::units::by_name(&s.ident)
    {
        return DynUnit::Known(expr);
    }
    DynUnit::Unknown {
        name: s.ident.clone(),
        docs: s.docs.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ww_si::units::{
        AngularVelocity, Degree, DegreeCelsius, Meter, Per, Ratio, Second, Speed, Times, Torque,
    };
    use ww_si::{Quantity, Unit};

    /// `Display` (both forms), `to_f64`, `to_si_f64` and the dimension of a typed quantity and of the dynamic one
    /// built from its parts.
    fn same<U: Unit>(q: Quantity<U>) {
        let d = DynQuantity {
            prefix: q.prefix,
            value: q.value,
            unit: DynUnit::from_expr(U::EXPR),
        };
        assert_eq!(d.to_string(), q.to_string());
        assert_eq!(format!("{d:#}"), format!("{q:#}"));
        assert_eq!(d.unit.symbol(), U::EXPR.symbol().to_string());
        assert_eq!(d.unit.name(), U::EXPR.name().to_string());
        assert_eq!(d.unit.dim(), Some(U::DIM));
        assert_eq!(d.to_f64(), q.to_f64());
        assert_eq!(d.to_si_f64(), Some(q.to_si_f64()));
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
    }
}

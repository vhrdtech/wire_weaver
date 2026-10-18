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
//! A value of `ww_si::AnyQuantity` (STD-37) carries its unit as an `SIExpr`: a named unit `ww_si` has becomes the same
//! [DynUnit] as the typed quantity's, a device's own named unit keeps the dimension and scale it sent.
//!
//! [DynQuantity] is a value with that unit: `3300 mV` as stored, `3.3 V` with `{:#}`, [DynQuantity::to_si_f64]. Units
//! are shown, named and converted by `ww_si::tree`, the code that does it for typed quantities.

use std::fmt;
use wire_weaver::shrink_wrap::{DeserializeShrinkWrap, RefBox, SerializeShrinkWrapOwned};
use ww_numeric::NumericValue;
use ww_self::{
    ApiBundleOwned, FieldsValueOwned, GenericArgOwned, InstanceOfOwned, ItemStructOwned, TypeOwned,
    ValueOwned,
};
use ww_si::tree::{self, Tree};
use ww_si::{AnyQuantity, AnyQuantityOwned, SIExpr, SIExprOwned, Scale};
pub use ww_si::{Dim, MAX_UNIT_DEPTH, Prefix, UnitExpr};

/// The unit of a `Quantity<U>`, built from `U` as introspection data describes it, or of an `AnyQuantity`, built
/// from the `SIExpr` the device sent with the value.
#[derive(Clone, Debug, PartialEq)]
pub enum DynUnit {
    /// A named unit of `ww_si::units`: symbol, dimension and scale are known.
    Known(&'static UnitExpr),
    /// A named unit sent by a device in an `AnyQuantity` that `ww_si` doesn't have (its own `Rpm`): symbol,
    /// dimension and scale as the device sent them.
    Named {
        name: String,
        symbol: String,
        dim: Dim,
        scale: Scale,
    },
    /// A marker `ww_si` doesn't know, a user's own (`struct Rpm;`): its name and docs, nothing else.
    Unknown { name: String, docs: Vec<String> },
    /// `units::Per<A, B>`, `A / B`.
    Per(Box<DynUnit>, Box<DynUnit>),
    /// `units::Times<A, B>`, `A · B`.
    Times(Box<DynUnit>, Box<DynUnit>),
    /// `A` to an integer power (`UnitExpr::Pow`, `SIExpr::Pow`).
    Pow(Box<DynUnit>, i8),
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
                        dim: *dim,
                        scale: *scale,
                    },
                },
                SIExpr::Mul(a, b) => DynUnit::Times(child(a), child(b)),
                SIExpr::Div(a, b) => DynUnit::Per(child(a), child(b)),
                SIExpr::Pow(a, n) => DynUnit::Pow(child(a), *n),
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
                dim: *dim,
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
    pub fn dim(&self) -> Option<Dim> {
        tree::dim(&Part::Dyn(self))
    }

    /// Factor to coherent SI (offsets left out, as in [UnitExpr::factor]), `None` with an unknown marker in it.
    pub fn factor(&self) -> Option<f64> {
        tree::factor(&Part::Dyn(self))
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

impl Tree for Part<'_> {
    fn node(&self) -> Option<tree::Node<'_, Self>> {
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
                    dim: Some(*dim),
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

/// A value of a `ww_si::Quantity<U>`: the prefix and number the device sent, and the unit.
#[derive(Clone, Debug, PartialEq)]
pub struct DynQuantity {
    pub prefix: Prefix,
    pub value: NumericValue,
    pub unit: DynUnit,
}

impl DynQuantity {
    /// `value` of type `ty` as a quantity, `None` when `ty` is not a `ww_si::Quantity<U>` or `ww_si::AnyQuantity`, or
    /// the value doesn't match it.
    pub fn from_value(value: &ValueOwned, ty: &TypeOwned, bundle: &ApiBundleOwned) -> Option<Self> {
        if is_any_quantity(ty, bundle) {
            // decoded by the derived type, the unit tree walked with its depth bound
            let bytes = value.ser_shrink_wrap_dyn(ty, bundle).ok()?;
            let q = AnyQuantity::from_ww_bytes(&bytes).ok()?;
            return Some(DynQuantity {
                prefix: q.prefix,
                value: q.value,
                unit: DynUnit::from_si_expr(&q.unit),
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
        Some(DynQuantity {
            prefix: prefix?,
            value: number?,
            unit,
        })
    }

    /// The value of `ty` (a `ww_si::Quantity` of the same unit, or an `AnyQuantity` with this unit) this quantity is,
    /// for writing it.
    pub fn to_value(&self, ty: &TypeOwned, bundle: &ApiBundleOwned) -> Result<ValueOwned, String> {
        if is_any_quantity(ty, bundle) {
            let Some(unit) = self.unit.to_si_expr() else {
                return Err(format!(
                    "a quantity in {} has no dimension and scale to send in an AnyQuantity",
                    self.unit.name()
                ));
            };
            let bytes = AnyQuantityOwned {
                prefix: self.prefix,
                value: self.value,
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

/// A quantity written as text, `3300 mV`, `3.3 V`, `-5 V`, `1.2e-4 N·m/(rad/s)`: a number, then the unit's symbol with
/// a prefix in front of it or none. Spaces don't matter, `*` stands for `·` and `u` for `µ`. `unit` is the unit the
/// value must be in (a `Quantity<U>`), the number alone is in the unit itself; `None` (an `AnyQuantity`) takes any
/// named unit of `ww_si` by its symbol. Integers are stored as `i32` (`i64` when larger), other numbers as `f32` when
/// that keeps the number as written, else `f64`.
pub fn parse_quantity(text: &str, unit: Option<&DynUnit>) -> Result<DynQuantity, String> {
    let squeezed: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    let (number, symbol) = squeezed.split_at(number_len(&squeezed));
    let symbol = symbol.replace('*', "·");
    let value = number_value(number).ok_or_else(|| format!("`{text}`: expected a number first"))?;
    let (prefix, unit) = match unit {
        Some(unit) => {
            let prefix = prefix_before(&symbol, &unit.symbol()).ok_or_else(|| {
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
                    prefix_before(&symbol, s).map(|p| (p, DynUnit::Known(expr)))
                }
                _ => None,
            })
            // `mV` is milli volt, not a unit called `mV`: the unit without a prefix wins, then the longest symbol
            .max_by_key(|(p, u)| (*p == Prefix::Unit, u.symbol().len()))
            .ok_or_else(|| format!("`{text}`: no unit of ww_si has the symbol `{symbol}`"))?,
    };
    Ok(DynQuantity {
        prefix,
        value,
        unit,
    })
}

/// The prefix `symbol` has in front of `unit`'s symbol: `Unit` when it is that symbol (or empty).
fn prefix_before(symbol: &str, unit: &str) -> Option<Prefix> {
    let unit = unit.replace(' ', "");
    if symbol == unit || symbol.is_empty() {
        return Some(Prefix::Unit);
    }
    let head = symbol.strip_suffix(unit.as_str())?;
    if head == "u" {
        return Some(Prefix::Micro);
    }
    Prefix::ALL
        .into_iter()
        .find(|p| *p != Prefix::Unit && p.symbol() == head)
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
/// `ww_si::Quantity`'s `Display`, written by the same code.
impl fmt::Display for DynQuantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(
            &tree::quantity(self.prefix, &self.value, &Part::Dyn(&self.unit)),
            f,
        )
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

/// Whether `ty` is a `ww_si::Quantity<U>`, see [unit_of], or a `ww_si::AnyQuantity`.
pub fn is_quantity(ty: &TypeOwned, bundle: &ApiBundleOwned) -> bool {
    unit_of(ty, bundle).is_some() || is_any_quantity(ty, bundle)
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

/// `bundle.types[type_idx]` is `ww_si::<path>::<ident>` of ww_si 0.2 or later.
fn is_ww_si(type_idx: u32, path: Option<&[&str]>, ident: &str, bundle: &ApiBundleOwned) -> bool {
    let Ok((TypeOwned::Struct(s), _)) = bundle.get_ty(type_idx) else {
        return false;
    };
    is_ww_si_struct(s, path, ident, bundle)
}

/// `s` is `ww_si::<path>::<ident>` of ww_si 0.2 or later.
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
    use wire_weaver::shrink_wrap::SerializeShrinkWrap;
    use ww_si::units::{
        AngularVelocity, Degree, DegreeCelsius, Meter, Per, Ratio, Second, Speed, Times, Torque,
    };

    #[test]
    fn quantities_from_text() {
        let any = |text: &str| {
            let q = parse_quantity(text, None).unwrap();
            (q.prefix, q.value, q.unit.symbol())
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
        const DIM: ww_si::Dim = ww_si::Dim::new(0, 2, 0, 0, 0, 0, 0);
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

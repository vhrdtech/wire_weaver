//! Units and scales of a bundle, from the definitions it carries (INT-13): a unit type of an API (`Volt`,
//! `Per<Meter, Second>`, a user's `Rpm`) resolved to its dimension, its exact factor to coherent SI and a canonical
//! expression, the sorted product of the named units in it, so that a client knows the units of any crate without a
//! table of its own.
//!
//! A marker struct carries its [definition](crate::ItemStructExtra::marker): a named [unit](crate::MarkerDef::Unit) or
//! [scale](crate::MarkerDef::Scale), or one of `exact_units`' generic markers (`Per<A, B>`, `Pow10<E>`, ...) whose
//! arguments say the rest. [unit_of] and [scale_of] walk a type through them, in a bundle as received or
//! [prepared](crate::generics::prepare).
//!
//! The canonical expression is F#'s: `Per<Meter, Second>`, `Times<Per<Ratio, Second>, Meter>` and a user's own
//! `Per<Meter, Second>` re-declared elsewhere are the same `m·s⁻¹` when they name the same markers with the same
//! powers. Two units of the same [dimension](CanonicalUnit::dim) convert into each other by the quotient of their
//! [magnitudes](CanonicalUnit::magnitude) even when their expressions differ (`km/h` and `m/s`), when they are of
//! the same [quantity](CanonicalUnit::quantity) (STD-68, UNIT-8): `Per<Volt, Volt>` is a voltage gain, not a
//! ratio, `Per<Volt, Ampere>` is not an `Ohm`, and `Of<Height, Meter>` converts to a length but not to a width, by
//! `exact_units`' rules ([exact_units::QuantityExpr]) with the bundle's [quantity](crate::MarkerDef::Quantity)
//! definitions as their table.

use crate::{
    ApiBundleOwned, GenericArgOwned, ItemStructOwned, MarkerDefOwned, OriginDefOwned, TypeOwned,
    ValueOwned,
};
use anyhow::{Context, Result, anyhow, bail};
use exact_units::{
    DimOwned, Magnitude, MarkerIdOwned, Offset, QuantityDefOwned, QuantityExpr, QuantityRef,
    UnitDefOwned, UnitTable,
};
use std::fmt;

/// Deepest nesting of `Per`, `Times`, `Sqrt`, full-scale, `Product` and `Quotient` markers [unit_of] and [scale_of] follow: a bundle from a
/// device is untrusted, a deeper unit is an error.
pub const MAX_UNIT_NESTING: usize = 16;

/// A named unit or scale marker: its crate and its name in it (`exact_units`, `units::Volt`), the identity a
/// marker has across bundles (D16b).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MarkerName {
    pub crate_id: String,
    /// The path in the crate and the name, as [ItemStructOwned::qualified_name] writes it.
    pub name: String,
}

impl fmt::Display for MarkerName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}", self.crate_id, self.name)
    }
}

/// The quantity a unit measures beyond its dimension (STD-68, UNIT-8), owned: an [exact_units::QuantityExpr] read
/// from a bundle. A dimension's open root (`Length`, `Energy`) is always [Self::Derived], so two of these are the
/// same quantity exactly when they are equal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QuantityExprOwned {
    /// A quantity of the tree other than an open root, by its id: `Ohm`'s resistance, `Of<Height, Meter>`'s height.
    Spec(MarkerIdOwned),
    /// Dimension one, a ratio of two quantities of this dimension: `Per<Volt, Volt>`.
    RatioOf(Box<DimOwned>),
    /// The quantity of a dimension and nothing more: `Meter`, `Per<Volt, Ampere>`.
    Derived(Box<DimOwned>),
}

impl QuantityExprOwned {
    /// The quantity as `exact_units` has it, for its rules: a spec by its [id](QuantityRef::Id), resolved through a
    /// table.
    pub fn as_expr(&self) -> QuantityExpr<'_> {
        match self {
            QuantityExprOwned::Spec(id) => QuantityExpr::Spec(QuantityRef::Id(id.as_id())),
            QuantityExprOwned::RatioOf(d) => QuantityExpr::RatioOf(d.as_dim()),
            QuantityExprOwned::Derived(d) => QuantityExpr::Derived(d.as_dim()),
        }
    }

    /// `e` owned, a spec looked up in `table`: an open root becomes [Self::Derived]; an error for a spec the table
    /// doesn't list.
    pub fn resolve(e: QuantityExpr<'_>, table: &dyn UnitTable) -> Result<Self> {
        Ok(match e {
            QuantityExpr::Spec(r) => {
                let def = r
                    .resolve(table)
                    .ok_or_else(|| anyhow!("the quantity {} is not in the bundle", r.id()))?;
                match def.is_open_root() {
                    true => QuantityExprOwned::Derived(Box::new(def.dim.into())),
                    false => QuantityExprOwned::Spec(def.id.into()),
                }
            }
            QuantityExpr::RatioOf(d) => QuantityExprOwned::RatioOf(Box::new(d.into())),
            QuantityExpr::Derived(d) => QuantityExprOwned::Derived(Box::new(d.into())),
        })
    }

    /// Whether it is [Self::Derived]: nothing beyond the dimension.
    pub fn is_derived(&self) -> bool {
        matches!(self, QuantityExprOwned::Derived(_))
    }
}

/// `resistance`, `ratio of V` (by the dimension), `derived`.
impl fmt::Display for QuantityExprOwned {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            QuantityExprOwned::Derived(_) => f.write_str("derived"),
            q => fmt::Display::fmt(&q.as_expr(), f),
        }
    }
}

/// One named unit in a [CanonicalUnit] and its power.
#[derive(Clone, Debug)]
pub struct UnitPower {
    pub marker: MarkerName,
    /// The unit's definition: its identity, symbol (`V`, `rpm`), dimension and scale.
    pub def: UnitDefOwned,
    /// The power doubled: 2 for `m`, -4 for `s⁻²`, 1 for `√Hz`. Never 0.
    pub exp_x2: i16,
}

/// A unit type resolved through the definitions of a bundle, see the [module docs](self).
#[derive(Clone, Debug)]
pub struct CanonicalUnit {
    /// The named units it is a product of, sorted by [MarkerName], each once. Empty for a ratio.
    pub powers: Vec<UnitPower>,
    pub dim: DimOwned,
    /// The exact factor to the coherent SI unit of [Self::dim].
    pub magnitude: Magnitude,
    /// The quantity beyond the dimension: what a quotient of one dimension kept, a named unit's own quantity, the
    /// `Q` of an `Of<Q, U>`.
    pub quantity: QuantityExprOwned,
}

/// The same unit: the same named units to the same powers, of the same quantity (`Per<Volt, Volt>` is no ratio,
/// though both have no powers left). `km/h` and `m/s` are not the same, they are
/// [convertible](CanonicalUnit::factor_to).
impl PartialEq for CanonicalUnit {
    fn eq(&self, other: &Self) -> bool {
        self.quantity == other.quantity
            && self.powers.len() == other.powers.len()
            && self
                .powers
                .iter()
                .zip(&other.powers)
                .all(|(a, b)| a.marker == b.marker && a.exp_x2 == b.exp_x2)
    }
}

impl CanonicalUnit {
    /// A ratio: no named units, dimension one, factor one.
    pub fn one() -> Self {
        CanonicalUnit {
            powers: Vec::new(),
            dim: exact_units::Dim::ONE.into(),
            magnitude: Magnitude::ONE,
            quantity: QuantityExprOwned::Derived(Box::new(exact_units::Dim::ONE.into())),
        }
    }

    /// The named unit this is, when it is one to the power one: `Kelvin`, not `Per<Meter, Second>` or a ratio.
    pub fn named(&self) -> Option<&UnitDefOwned> {
        match self.powers.as_slice() {
            [p] if p.exp_x2 == 2 => Some(&p.def),
            _ => None,
        }
    }

    /// The exact factor a number in this unit is multiplied by to be in `other`, `None` when the dimensions differ
    /// or the quantity doesn't [convert](QuantityExpr::converts) (`table`, usually the bundle, lists the quantities
    /// of both): 1/1000 from `m` to `km`, π/30 from `rpm` to `rad/s`, 1 from a height in m to a length in m; none
    /// from V/V to a ratio, nor from a width to a height.
    pub fn factor_to(&self, other: &CanonicalUnit, table: &dyn UnitTable) -> Option<Magnitude> {
        if !self.dim.as_dim().same(other.dim.as_dim())
            || !self
                .quantity
                .as_expr()
                .converts(other.quantity.as_expr(), table)?
        {
            return None;
        }
        self.magnitude.checked_div(other.magnitude).ok()
    }

    fn checked_mul(self, other: CanonicalUnit, sign: i16) -> Result<Self> {
        let (a, b) = (self.dim.as_dim(), other.dim.as_dim());
        let dim = match sign {
            1 => self.dim.checked_mul(&other.dim),
            _ => self.dim.checked_div(&other.dim),
        }
        .ok_or_else(|| anyhow!("dimension of {self} and {other} combined is out of range"))?;
        let (qa, qb) = (self.quantity.as_expr(), other.quantity.as_expr());
        // the rules give a spec of an input or a derived/ratio quantity, never a new spec: no table needed
        let quantity = match match sign {
            1 => qa.mul(a, qb, b, dim.as_dim()),
            _ => qa.div(a, qb, b, dim.as_dim()),
        } {
            QuantityExpr::Spec(r) => QuantityExprOwned::Spec(r.id().into()),
            QuantityExpr::RatioOf(d) => QuantityExprOwned::RatioOf(Box::new(d.into())),
            QuantityExpr::Derived(d) => QuantityExprOwned::Derived(Box::new(d.into())),
        };
        let magnitude = match sign {
            1 => self.magnitude.checked_mul(other.magnitude),
            _ => self.magnitude.checked_div(other.magnitude),
        }
        .map_err(|e| anyhow!("factor of {self} and {other} combined: {e:?}"))?;
        let mut powers = self.powers;
        for p in other.powers {
            let exp_x2 = p
                .exp_x2
                .checked_mul(sign)
                .ok_or_else(|| anyhow!("power of {} out of range", p.marker))?;
            match powers.binary_search_by(|q| q.marker.cmp(&p.marker)) {
                Ok(i) => {
                    powers[i].exp_x2 = powers[i]
                        .exp_x2
                        .checked_add(exp_x2)
                        .ok_or_else(|| anyhow!("power of {} out of range", p.marker))?;
                    if powers[i].exp_x2 == 0 {
                        powers.remove(i);
                    }
                }
                Err(i) => powers.insert(i, UnitPower { exp_x2, ..p }),
            }
        }
        Ok(CanonicalUnit {
            powers,
            dim,
            magnitude,
            quantity,
        })
    }

    fn checked_sqrt(self) -> Result<Self> {
        let dim = self
            .dim
            .checked_sqrt()
            .ok_or_else(|| anyhow!("square root of {self}, an odd power of a half"))?;
        let magnitude = self
            .magnitude
            .checked_sqrt()
            .map_err(|e| anyhow!("square root of the factor of {self}: {e:?}"))?;
        let mut powers = self.powers;
        for p in &mut powers {
            if p.exp_x2 % 2 != 0 {
                bail!("square root of {}, which already is a root", p.marker);
            }
            p.exp_x2 /= 2;
        }
        let quantity = QuantityExprOwned::Derived(Box::new(dim.clone()));
        Ok(CanonicalUnit {
            powers,
            dim,
            magnitude,
            quantity,
        })
    }
}

/// The canonical expression with the units' symbols, in the order of their markers: `m·s⁻¹`, `(rad/s)⁻¹·N·m` for
/// `Per<Torque, AngularVelocity>` (both named units of their own), `V·Hz^(-1/2)`, `1` for a ratio.
impl fmt::Display for CanonicalUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.powers.is_empty() {
            return f.write_str("1");
        }
        for (i, p) in self.powers.iter().enumerate() {
            if i > 0 {
                f.write_str("·")?;
            }
            // a compound symbol to a power in parentheses: `(rad/s)⁻¹`
            let symbol = &p.def.names.symbol;
            match p.exp_x2 != 2 && symbol.contains(['·', '/', ' ']) {
                true => write!(f, "({symbol})")?,
                false => f.write_str(symbol)?,
            }
            match (p.exp_x2 % 2 == 0, p.exp_x2 / 2) {
                (true, 1) => {}
                (true, e) => write_superscript(f, e)?,
                (false, _) if p.exp_x2 == 1 => f.write_str("^½")?,
                (false, _) => write!(f, "^({}/2)", p.exp_x2)?,
            }
        }
        Ok(())
    }
}

fn write_superscript(f: &mut fmt::Formatter<'_>, e: i16) -> fmt::Result {
    const DIGITS: [char; 10] = ['⁰', '¹', '²', '³', '⁴', '⁵', '⁶', '⁷', '⁸', '⁹'];
    if e < 0 {
        f.write_str("⁻")?;
    }
    for c in e.unsigned_abs().to_string().bytes() {
        write!(f, "{}", DIGITS[(c - b'0') as usize])?;
    }
    Ok(())
}

/// The unit `ty` is, through the [definitions](crate::ItemStructExtra::marker) in `bundle`: a named unit, or `Per`,
/// `Times`, `Sqrt` of units. An error for a type that is not a unit marker, or a marker without a definition (a crate
/// whose markers are hand-written).
pub fn unit_of(ty: &TypeOwned, bundle: &ApiBundleOwned) -> Result<CanonicalUnit> {
    unit_in(ty, bundle, 0)
}

/// The exact factor of the scale marker `ty` is (the `S` of a `Fixed<U, S, ..>`): a named scale (`prefix::Milli`, a
/// user's `scale!`), `Pow10<E>`, `Pow2<E>`, `Rational<NUM, DEN>`, `FullScale<V, BITS>`, `FullScaleMax<V, BITS>`, or
/// `Product<A, B>` / `Quotient<A, B>` of two scales (exact, an error where the factor doesn't fit a `Magnitude`).
pub fn scale_of(ty: &TypeOwned, bundle: &ApiBundleOwned) -> Result<Magnitude> {
    scale_in(ty, bundle, 0)
}

/// The symbol a scale marker writes before a unit's symbol: `m` for `prefix::Milli`, `Ki` for `Kibi`, a user's
/// `scale!` symbol; empty for a scale without one (`Pow2<-15>`, `Rational<1, 3>`), which is shown as its factor.
pub fn scale_symbol_of(ty: &TypeOwned, bundle: &ApiBundleOwned) -> Result<String> {
    let (s, _) = marker(ty, bundle)?;
    Ok(match definition(s, bundle)? {
        MarkerDefOwned::Scale(def) => def.symbol.clone(),
        _ => String::new(),
    })
}

/// The number type a wrapper stores (the `T` of a `Fixed`, `Index`, `IndexFrom1` or `Seq`).
pub fn storage_of(ty: &TypeOwned, bundle: &ApiBundleOwned) -> Result<ww_numeric::NumericBaseType> {
    match ty.get_in_line(bundle)? {
        TypeOwned::NumericAny(any) => Ok(crate::value::numeric_base(any).clone()),
        other => bail!("{} is not a number type", ty_name(other, bundle)),
    }
}

/// The origin `ty` is (the `O` of a `Point<O, F>`): an `exact_units::origin!` marker, whose definition is written out
/// whole, or `Angle<U>`, an angle from the zero of the unit `U` that wraps every turn.
pub fn origin_of(ty: &TypeOwned, bundle: &ApiBundleOwned) -> Result<OriginDefOwned> {
    let (s, args) = marker(ty, bundle)?;
    match definition(s, bundle)? {
        MarkerDefOwned::Origin(def) => Ok(def.clone()),
        MarkerDefOwned::Angle => {
            let unit = match args.first() {
                Some(GenericArgOwned::Type(u)) => unit_of(u, bundle)?,
                _ => bail!("{} without its unit", s.ident),
            };
            if !is_plane_angle(&unit.dim) {
                bail!("Angle<{unit}>: {unit} is not a plane angle");
            }
            // a turn, 2π rad, in the unit
            let turn = Magnitude::ratio(2, 1)
                .checked_mul(Magnitude::PI)
                .and_then(|t| t.checked_div(unit.magnitude))
                .map_err(|e| anyhow!("a turn in {unit}: {e:?}"))?;
            let Some(def) = unit.named() else {
                bail!("Angle<{unit}>: an angle's unit is a named unit");
            };
            Ok(OriginDefOwned {
                id: MarkerIdOwned {
                    crate_name: "exact_units".into(),
                    name: "Angle".into(),
                },
                unit: def.clone(),
                symbol: unit.to_string(),
                reference: unit_zero(),
                offset: Offset::ZERO,
                modulus: Some(turn),
                step: Magnitude::ONE,
                shown: Offset::ZERO,
            })
        }
        _ => bail!("{} is not an origin", marker_name(s, bundle)?),
    }
}

/// The identity of a unit, scale or origin definition (`exact_units::MarkerId`), `None` for `exact_units`' generic
/// markers, which are known by their variant.
pub fn def_id(def: &MarkerDefOwned) -> Option<&MarkerIdOwned> {
    match def {
        MarkerDefOwned::Unit(u) | MarkerDefOwned::Base(u) => Some(&u.id),
        MarkerDefOwned::Scale(s) => Some(&s.id),
        MarkerDefOwned::Origin(o) => Some(&o.id),
        _ => None,
    }
}

/// Refuses a bundle with two different definitions under one identity (UNIT-7): a marker is known by its crate and
/// name, so two versions of one crate whose `Rpm`s differ, or a hand-edited model, would give one unit two meanings.
pub fn check_marker_ids(bundle: &ApiBundleOwned) -> Result<()> {
    let mut seen = std::collections::HashMap::new();
    for location in &bundle.types {
        let crate::TypeLocationOwned::InLine {
            ty: TypeOwned::Struct(s),
            ..
        } = location
        else {
            continue;
        };
        let Some(def) = s.marker() else {
            continue;
        };
        let Some(id) = def_id(def) else {
            continue;
        };
        if let Some(first) = seen.insert(id, def)
            && first != def
        {
            bail!(
                "marker {} has two definitions in the model: {first:?} and {def:?}",
                id.as_id()
            );
        }
    }
    Ok(())
}

/// The bundle as an [exact_units::UnitTable]: the definition of every named unit whose marker it carries, by its
/// identity. The schema lists every unit the API uses, `exact_units`' own too, so an `SIExpr` reference
/// (`SIExpr::Ref`) a device sends beside its API resolves through the bundle alone, with no crate compiled in.
impl exact_units::UnitTable for ApiBundleOwned {
    fn unit(&self, id: exact_units::MarkerId<'_>) -> Option<exact_units::UnitDef<'_>> {
        self.types.iter().find_map(|location| match location {
            crate::TypeLocationOwned::InLine {
                ty: TypeOwned::Struct(s),
                ..
            } => s
                .marker()?
                .unit_def()
                .filter(|def| def.id.as_id() == id)
                .map(UnitDefOwned::as_def),
            _ => None,
        })
    }

    fn quantity(&self, id: exact_units::MarkerId<'_>) -> Option<exact_units::QuantityDef<'_>> {
        self.types.iter().find_map(|location| match location {
            crate::TypeLocationOwned::InLine {
                ty: TypeOwned::Struct(s),
                ..
            } => s
                .marker()?
                .quantity_def()
                .filter(|def| def.id.as_id() == id)
                .map(QuantityDefOwned::as_def),
            _ => None,
        })
    }
}

/// `exact_units::UnitZero`, the reference of an origin measured from its unit's own zero.
pub fn unit_zero() -> MarkerIdOwned {
    MarkerIdOwned {
        crate_name: "exact_units".into(),
        name: "UnitZero".into(),
    }
}

/// The identity of the marker struct `ty` is (a generic one's definition for an instance): its crate as Rust names
/// it (`my_api`, never `my-api`) and its ident, without the module path, as `exact_units::MarkerId` has it.
pub fn marker_id_of(ty: &TypeOwned, bundle: &ApiBundleOwned) -> Result<MarkerIdOwned> {
    let (s, _) = marker(ty, bundle)?;
    Ok(MarkerIdOwned {
        crate_name: bundle.crate_name(s.crate_idx.0)?.replace('-', "_"),
        name: s.ident.clone(),
    })
}

/// The definition of the quantity marker `ty` (`exact_units::quantity!`, UNIT-8): `Height`, `Resistance`.
pub fn quantity_of(ty: &TypeOwned, bundle: &ApiBundleOwned) -> Result<QuantityDefOwned> {
    let (s, _) = marker(ty, bundle)?;
    match definition(s, bundle)? {
        MarkerDefOwned::Quantity(def) => Ok(def.clone()),
        _ => bail!("{} is not a quantity", marker_name(s, bundle)?),
    }
}

/// The index in `bundle` of the quantity marker `id` (UNIT-8): its definition in line, or skipped under its name
/// (`quantities::Resistance` of crate `exact_units`). A marker that names a quantity by id
/// ([MarkerDefOwned::quantity_ref]) keeps it in the bundle, as a reference to a type would.
pub fn quantity_type_idx(bundle: &ApiBundleOwned, id: &MarkerIdOwned) -> Option<u32> {
    let of_crate = |crate_idx: u32| {
        bundle
            .crate_name(crate_idx)
            .is_ok_and(|name| name.replace('-', "_") == id.crate_name)
    };
    let idx = bundle.types.iter().position(|location| match location {
        crate::TypeLocationOwned::InLine {
            ty: TypeOwned::Struct(s),
            ..
        } => s
            .marker()
            .and_then(MarkerDefOwned::quantity_def)
            .is_some_and(|def| def.id == *id),
        crate::TypeLocationOwned::SkippedFullVersion {
            crate_idx,
            type_name,
            ..
        } => of_crate(crate_idx.0) && type_name.rsplit("::").next() == Some(id.name.as_str()),
        _ => false,
    })?;
    Some(idx as u32)
}

/// The crate and name of the marker struct `ty` is (a generic one's definition for an instance).
pub fn marker_name_of(ty: &TypeOwned, bundle: &ApiBundleOwned) -> Result<MarkerName> {
    marker_name(marker(ty, bundle)?.0, bundle)
}

/// The marker struct `ty` is, with the generic definition and arguments it is an instance of (empty for a
/// non-generic one).
fn marker<'a>(
    ty: &'a TypeOwned,
    bundle: &'a ApiBundleOwned,
) -> Result<(&'a ItemStructOwned, &'a [GenericArgOwned])> {
    let def = |type_idx: u32| match bundle.get_ty(type_idx)? {
        (TypeOwned::Struct(s), _) => Ok(s),
        _ => bail!("{} is not a marker struct", ty_name(ty, bundle)),
    };
    match ty {
        TypeOwned::Instance { type_idx, args } => Ok((def(type_idx.0)?, args)),
        TypeOwned::OutOfLine { type_idx } => {
            let s = def(type_idx.0)?;
            // a concrete definition of a prepared bundle: the generic one says what it is
            match &s.instance_of {
                Some(inst) => Ok((def(inst.type_idx.0)?, &inst.args)),
                None => Ok((s, &[])),
            }
        }
        TypeOwned::Struct(s) => Ok((s, &[])),
        _ => bail!("{} is not a marker struct", ty_name(ty, bundle)),
    }
}

fn ty_name(ty: &TypeOwned, bundle: &ApiBundleOwned) -> String {
    ty.human_name(true, bundle)
        .unwrap_or_else(|_| "a type".into())
}

fn marker_name(s: &ItemStructOwned, bundle: &ApiBundleOwned) -> Result<MarkerName> {
    Ok(MarkerName {
        crate_id: bundle.crate_name(s.crate_idx.0)?.to_string(),
        name: s.qualified_name(),
    })
}

fn definition<'a>(s: &'a ItemStructOwned, bundle: &ApiBundleOwned) -> Result<&'a MarkerDefOwned> {
    s.marker().ok_or_else(|| {
        anyhow!(
            "{} has no unit, scale or origin definition",
            marker_name(s, bundle)
                .map(|n| n.to_string())
                .unwrap_or_else(|_| s.ident.clone())
        )
    })
}

fn unit_in(ty: &TypeOwned, bundle: &ApiBundleOwned, depth: usize) -> Result<CanonicalUnit> {
    if depth > MAX_UNIT_NESTING {
        bail!("unit nested deeper than {MAX_UNIT_NESTING}");
    }
    let (s, args) = marker(ty, bundle)?;
    let arg = |i: usize| match args.get(i) {
        Some(GenericArgOwned::Type(t)) => unit_in(t, bundle, depth + 1),
        _ => bail!("{} without its unit arguments", s.ident),
    };
    match definition(s, bundle)? {
        // `Ratio`: the unit of a plain number, no factor of a product (`Times<Per<Ratio, Second>, Meter>` is m·s⁻¹)
        MarkerDefOwned::Unit(def)
            if def.names.symbol.is_empty()
                && def.dim.as_dim().is_one()
                && def.quantity.is_none()
                && def.scale == Magnitude::ONE =>
        {
            Ok(CanonicalUnit::one())
        }
        MarkerDefOwned::Unit(def) | MarkerDefOwned::Base(def) => Ok(CanonicalUnit {
            powers: vec![UnitPower {
                marker: marker_name(s, bundle)?,
                def: def.clone(),
                exp_x2: 2,
            }],
            dim: def.dim.clone(),
            magnitude: def.scale,
            quantity: QuantityExprOwned::resolve(def.as_def().quantity_expr(), bundle)
                .with_context(|| format!("unit {}", def.id.as_id()))?,
        }),
        MarkerDefOwned::Per => arg(0)?.checked_mul(arg(1)?, -1),
        MarkerDefOwned::Times => arg(0)?.checked_mul(arg(1)?, 1),
        MarkerDefOwned::Sqrt => arg(0)?.checked_sqrt(),
        MarkerDefOwned::Of => {
            let q = match args.first() {
                Some(GenericArgOwned::Type(t)) => quantity_of(t, bundle)?,
                _ => bail!("{} without its quantity", s.ident),
            };
            let unit = arg(1)?;
            let name = || format!("Of<{}, {unit}>", q.id.name);
            if !q.dim.as_dim().same(unit.dim.as_dim()) {
                bail!("{}: the quantity's dimension is not the unit's", name());
            }
            if unit.quantity.as_expr().reaches(&q.as_def(), bundle) != Some(true) {
                bail!(
                    "{}: the unit's own quantity ({}) doesn't reach it (a sibling, or another kind)",
                    name(),
                    unit.quantity
                );
            }
            let quantity = QuantityExprOwned::resolve(
                QuantityExpr::Spec(QuantityRef::Def(&q.as_def())),
                bundle,
            )?;
            Ok(CanonicalUnit { quantity, ..unit })
        }
        _ => bail!("{} is not a unit", marker_name(s, bundle)?),
    }
}

fn scale_in(ty: &TypeOwned, bundle: &ApiBundleOwned, depth: usize) -> Result<Magnitude> {
    if depth > MAX_UNIT_NESTING {
        bail!("scale nested deeper than {MAX_UNIT_NESTING}");
    }
    let (s, args) = marker(ty, bundle)?;
    let int = |i: usize| match args.get(i) {
        Some(GenericArgOwned::Const(ValueOwned::Numeric(n))) => n
            .to_i128()
            .ok_or_else(|| anyhow!("{} with a parameter that is not an integer", s.ident)),
        _ => bail!("{} without its parameters", s.ident),
    };
    let small = |i: usize, max: i128| {
        let e = int(i)?;
        match (-max..=max).contains(&e) {
            true => Ok(e as i8),
            false => bail!("{} with a parameter out of -{max}..={max}", s.ident),
        }
    };
    let bits = |i: usize| {
        let b = int(i)?;
        match (1..=63).contains(&b) {
            true => Ok(b as u8),
            false => bail!("{} of {b} bits, 1 to 63 are possible", s.ident),
        }
    };
    let error = |e| anyhow!("{}: {e:?}", s.ident);
    let full_scale = || match args.first() {
        Some(GenericArgOwned::Type(v)) => scale_in(v, bundle, depth + 1),
        _ => bail!("{} without its full-scale value", s.ident),
    };
    let scale_arg = |i: usize| match args.get(i) {
        Some(GenericArgOwned::Type(p)) => scale_in(p, bundle, depth + 1),
        _ => bail!("{} without its scales", s.ident),
    };
    match definition(s, bundle)? {
        MarkerDefOwned::Scale(def) => Ok(def.magnitude),
        MarkerDefOwned::Pow10 => Magnitude::checked_pow10(small(0, 63)?)
            .ok_or_else(|| anyhow!("{} out of range", s.ident)),
        MarkerDefOwned::Pow2 => Ok(Magnitude::pow2(small(0, 63)?)),
        MarkerDefOwned::Rational => {
            let part =
                |i| u64::try_from(int(i)?).map_err(|_| anyhow!("{} with a negative part", s.ident));
            Magnitude::checked_ratio(part(0)?, part(1)?).map_err(error)
        }
        MarkerDefOwned::FullScale => full_scale()?
            .checked_div(Magnitude::pow2(bits(1)? as i8))
            .map_err(error),
        MarkerDefOwned::FullScaleMax => {
            let max_code = (1u64 << bits(1)?) - 1;
            full_scale()?
                .checked_div(Magnitude::checked_ratio(max_code, 1).map_err(error)?)
                .map_err(error)
        }
        MarkerDefOwned::Product => scale_arg(0)?.checked_mul(scale_arg(1)?).map_err(error),
        MarkerDefOwned::Quotient => scale_arg(0)?.checked_div(scale_arg(1)?).map_err(error),
        _ => bail!("{} is not a scale", marker_name(s, bundle)?),
    }
}

/// A number wrapper of `exact_units` (all `transparent`, the same bytes as their storage `T`): its parts as type
/// arguments of a [prepared](crate::generics::prepare) bundle.
#[derive(Clone, Copy, Debug)]
pub enum Wrapper<'a> {
    /// `Fixed<U, P, T, B>`: a number of scale `P` of unit `U`.
    Fixed {
        unit: &'a TypeOwned,
        scale: &'a TypeOwned,
        storage: &'a TypeOwned,
        bounds: &'a TypeOwned,
    },
    /// `Index<Of, T, B>`: a 0-based position in a table of `Of`.
    Index {
        of: &'a TypeOwned,
        storage: &'a TypeOwned,
        bounds: &'a TypeOwned,
    },
    /// `IndexFrom1<Of, T, B>`: a 1-based number of an `Of`, never 0.
    IndexFrom1 {
        of: &'a TypeOwned,
        storage: &'a TypeOwned,
        bounds: &'a TypeOwned,
    },
    /// `Seq<Of, T>`: a wrapping sequence number.
    Seq {
        of: &'a TypeOwned,
        storage: &'a TypeOwned,
    },
}

impl Wrapper<'_> {
    /// `Fixed`, `Index`, `IndexFrom1`, `Seq`.
    pub fn name(&self) -> &'static str {
        match self {
            Wrapper::Fixed { .. } => "Fixed",
            Wrapper::Index { .. } => "Index",
            Wrapper::IndexFrom1 { .. } => "IndexFrom1",
            Wrapper::Seq { .. } => "Seq",
        }
    }

    /// The `B` argument, `None` for a `Seq`.
    pub fn bounds_ty(&self) -> Option<&TypeOwned> {
        match self {
            Wrapper::Fixed { bounds, .. }
            | Wrapper::Index { bounds, .. }
            | Wrapper::IndexFrom1 { bounds, .. } => Some(bounds),
            Wrapper::Seq { .. } => None,
        }
    }
}

/// The `exact_units` wrapper `ty` is an instance of, `None` for any other type (or a wrong number of arguments).
pub fn wrapper_of<'a>(ty: &'a TypeOwned, bundle: &'a ApiBundleOwned) -> Option<Wrapper<'a>> {
    let s = match ty.get_in_line(bundle).ok()? {
        TypeOwned::Struct(s) => s,
        _ => return None,
    };
    wrapper_of_struct(s, bundle)
}

/// [wrapper_of] for a concrete struct definition of a prepared bundle.
pub fn wrapper_of_struct<'a>(
    s: &'a ItemStructOwned,
    bundle: &'a ApiBundleOwned,
) -> Option<Wrapper<'a>> {
    let inst = s.instance_of.as_ref()?;
    let TypeOwned::Struct(g) = bundle.get_ty(inst.type_idx.0).ok()?.0 else {
        return None;
    };
    if g.path.is_some() || bundle.crate_name(g.crate_idx.0).ok()? != "exact_units" {
        return None;
    }
    let mut args = inst.args.iter().map(|a| match a {
        GenericArgOwned::Type(t) => Some(t),
        _ => None,
    });
    let mut next = || args.next().flatten();
    let w = match g.ident.as_str() {
        "Fixed" => Wrapper::Fixed {
            unit: next()?,
            scale: next()?,
            storage: next()?,
            bounds: next()?,
        },
        "Index" => Wrapper::Index {
            of: next()?,
            storage: next()?,
            bounds: next()?,
        },
        "IndexFrom1" => Wrapper::IndexFrom1 {
            of: next()?,
            storage: next()?,
            bounds: next()?,
        },
        "Seq" => Wrapper::Seq {
            of: next()?,
            storage: next()?,
        },
        _ => return None,
    };
    Some(w)
}

/// One end of [NumBounds]: an integer, or a float for a float storage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Bound {
    Int(i128),
    Float(f64),
}

impl fmt::Display for Bound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Bound::Int(i) => write!(f, "{i}"),
            Bound::Float(x) => write!(f, "{x}"),
        }
    }
}

/// The numbers a bounds marker allows, as stored: `Range<1, 5000>` is `1..=5000`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NumBounds {
    pub min: Option<Bound>,
    pub max: Option<Bound>,
    /// Whether a float may be NaN: only for `MaybeNan` (NUM-19). Every other marker, `Unbounded` included, refuses
    /// NaN, as `exact_units` does on decode.
    pub nan: bool,
    // TODO(STD-104): exact_units' `Finite` (NUM-20), which refuses ±∞ too, once it exists there
}

impl NumBounds {
    /// No ends: `Unbounded`, `Wrapping`, `Saturating`, `MaybeNan`.
    pub fn is_none(&self) -> bool {
        self.min.is_none() && self.max.is_none()
    }

    /// Whether `n` is within: an integer against integer bounds exactly, a float against float ones as
    /// `exact_units` compares them. NaN is out unless the marker is `MaybeNan`.
    pub fn contains(&self, n: &ww_numeric::NumericValue) -> bool {
        if n.to_i128().is_none() && n.to_f64().is_nan() {
            return self.nan;
        }
        let above = |b: &Bound| match (b, n.to_i128()) {
            (Bound::Int(min), Some(i)) => i >= *min,
            (Bound::Int(min), None) => n.to_f64() >= *min as f64,
            (Bound::Float(min), _) => n.to_f64() >= *min,
        };
        let below = |b: &Bound| match (b, n.to_i128()) {
            (Bound::Int(max), Some(i)) => i <= *max,
            (Bound::Int(max), None) => n.to_f64() <= *max as f64,
            (Bound::Float(max), _) => n.to_f64() <= *max,
        };
        self.min.as_ref().is_none_or(above) && self.max.as_ref().is_none_or(below)
    }
}

/// `1..=5000`, `..=7`, `-0.5..=2.5`, empty without bounds.
impl fmt::Display for NumBounds {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_none() {
            return Ok(());
        }
        if let Some(min) = &self.min {
            write!(f, "{min}")?;
        }
        f.write_str("..=")?;
        if let Some(max) = &self.max {
            write!(f, "{max}")?;
        }
        Ok(())
    }
}

/// The bounds marker `ty` is (the `B` of a `Fixed`, `Index` or `IndexFrom1`): `Unbounded`, `Wrapping`,
/// `Saturating` and `MaybeNan` have no ends (only `MaybeNan` takes NaN), `Range<MIN, MAX>` and `RangeU64<MIN, MAX>` integer ends, `RangeDec<MIN, MAX, EXP>` the
/// float ends `MIN·10^EXP` and `MAX·10^EXP`.
pub fn bounds_of(ty: &TypeOwned, bundle: &ApiBundleOwned) -> Result<NumBounds> {
    let (s, args) = marker(ty, bundle)?;
    let int = |i: usize| match args.get(i) {
        Some(GenericArgOwned::Const(ValueOwned::Numeric(n))) => n
            .to_i128()
            .ok_or_else(|| anyhow!("{} with a parameter that is not an integer", s.ident)),
        _ => bail!("{} without its parameters", s.ident),
    };
    Ok(match definition(s, bundle)? {
        MarkerDefOwned::Unbounded | MarkerDefOwned::Wrapping | MarkerDefOwned::Saturating => {
            NumBounds::default()
        }
        MarkerDefOwned::MaybeNan => NumBounds {
            nan: true,
            ..NumBounds::default()
        },
        MarkerDefOwned::Range | MarkerDefOwned::RangeU64 => NumBounds {
            min: Some(Bound::Int(int(0)?)),
            max: Some(Bound::Int(int(1)?)),
            nan: false,
        },
        MarkerDefOwned::RangeDec => {
            let exp = i8::try_from(int(2)?)
                .map_err(|_| anyhow!("{} with an exponent out of range", s.ident))?;
            let end = |i| -> Result<Bound> {
                Ok(Bound::Float(
                    int(i)? as i64 as f64 * exact_units::num::pow10_f64(exp),
                ))
            };
            NumBounds {
                min: Some(end(0)?),
                max: Some(end(1)?),
                nan: false,
            }
        }
        _ => bail!("{} is not a bounds marker", marker_name(s, bundle)?),
    })
}

/// The rules an `exact_units` wrapper's number follows besides its storage, checked on every decode and encode: a
/// `Fixed`, `Index` or `IndexFrom1` within its bounds, an `IndexFrom1` never 0. `fields` is the value of the
/// transparent struct `s`; anything that isn't such a wrapper passes.
pub fn check_wrapper(
    s: &ItemStructOwned,
    fields: &crate::FieldsValueOwned,
    bundle: &ApiBundleOwned,
) -> Result<()> {
    let Some(w) = wrapper_of_struct(s, bundle) else {
        return Ok(());
    };
    let number = match fields {
        crate::FieldsValueOwned::Named(f) => f.first().map(|(_, v)| v),
        crate::FieldsValueOwned::Unnamed(f) => f.first(),
        crate::FieldsValueOwned::Unit => None,
    };
    let Some(ValueOwned::Numeric(n)) = number else {
        return Ok(());
    };
    let name = || ty_name(&TypeOwned::Struct(s.clone()), bundle);
    if matches!(w, Wrapper::IndexFrom1 { .. }) && n.to_i128() == Some(0) {
        bail!("0 is no {}: it counts from 1", name());
    }
    if let Some(b) = w.bounds_ty() {
        let bounds = bounds_of(b, bundle)?;
        if !bounds.contains(n) && !bounds.nan && n.to_i128().is_none() && n.to_f64().is_nan() {
            bail!(
                "NaN is no value of {}: only a MaybeNan float holds NaN",
                name()
            );
        }
        if !bounds.contains(n) {
            let shown = match n.to_i128() {
                Some(i) => i.to_string(),
                None => n.to_f64().to_string(),
            };
            bail!("{shown} is out of {} ({bounds})", name());
        }
    }
    Ok(())
}

/// rad¹ and nothing else: the dimension of an `Angle<U>`'s unit.
fn is_plane_angle(dim: &DimOwned) -> bool {
    dim.checked_div(&<exact_units::units::Radian as exact_units::Unit>::DIM.into())
        .is_some_and(|rest| rest.as_dim().is_one())
}

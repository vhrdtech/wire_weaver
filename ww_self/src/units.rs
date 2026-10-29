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
//! the same [kind](CanonicalUnit::kind) (STD-68): `Per<Volt, Volt>` is a voltage gain, not a ratio, and
//! `Per<Volt, Ampere>` is not an `Ohm`, by `exact_units`' rules ([exact_units::Kind]).

use crate::{
    ApiBundleOwned, ConventionalUnitDefOwned, GenericArgOwned, ItemStructOwned, KindUnitDefOwned,
    MarkerDefOwned, OriginDefOwned, TypeOwned, UnitDefOwned, ValueOwned,
};
use anyhow::{Result, anyhow, bail};
use exact_units::{DimOwned, Kind, Magnitude, Offset};
use std::fmt;

/// Deepest nesting of `Per`, `Times`, `Sqrt` and full-scale markers [unit_of] and [scale_of] follow: a bundle from a
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

/// The kind of quantity of a unit beyond its dimension (STD-68), owned: an [exact_units::Kind] read from a bundle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnitKind {
    /// No kind beyond the dimension.
    Plain,
    /// A kind of its own (`resistance`), from a [KindUnit](MarkerDefOwned::KindUnit).
    Named(String),
    /// Dimension one, a ratio of two quantities of this dimension: `Per<Volt, Volt>`.
    RatioOf(Box<DimOwned>),
}

impl UnitKind {
    /// The kind as `exact_units` has it, for its rules.
    pub fn as_kind(&self) -> Kind<'_> {
        match self {
            UnitKind::Plain => Kind::Plain,
            UnitKind::Named(n) => Kind::Named(n),
            UnitKind::RatioOf(d) => Kind::RatioOf(d.as_dim()),
        }
    }
}

impl From<Kind<'_>> for UnitKind {
    fn from(k: Kind<'_>) -> Self {
        match k {
            Kind::Plain => UnitKind::Plain,
            Kind::Named(n) => UnitKind::Named(n.to_string()),
            Kind::RatioOf(d) => UnitKind::RatioOf(Box::new(d.into())),
        }
    }
}

/// `resistance`, `ratio of V` (by the dimension), `plain`.
impl fmt::Display for UnitKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UnitKind::Plain => f.write_str("plain"),
            kind => fmt::Display::fmt(&kind.as_kind(), f),
        }
    }
}

/// One named unit in a [CanonicalUnit] and its power.
#[derive(Clone, Debug)]
pub struct UnitPower {
    pub marker: MarkerName,
    /// What the unit's definition writes after a number: `V`, `rpm`.
    pub symbol: String,
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
    /// The kind beyond the dimension: what a quotient of one dimension kept, a named unit's own kind.
    pub kind: UnitKind,
}

/// The same unit: the same named units to the same powers, of the same kind (`Per<Volt, Volt>` is no ratio,
/// though both have no powers left). `km/h` and `m/s` are not the same, they are
/// [convertible](CanonicalUnit::factor_to).
impl PartialEq for CanonicalUnit {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
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
            kind: UnitKind::Plain,
        }
    }

    /// The exact factor a number in this unit is multiplied by to be in `other`, `None` when the dimensions or the
    /// kinds differ: 1/1000 from `m` to `km`, π/30 from `rpm` to `rad/s`; none from V/V to a ratio.
    pub fn factor_to(&self, other: &CanonicalUnit) -> Option<Magnitude> {
        if !self.dim.as_dim().same(other.dim.as_dim()) || self.kind != other.kind {
            return None;
        }
        self.magnitude.checked_div(other.magnitude).ok()
    }

    fn checked_mul(self, other: CanonicalUnit, sign: i16) -> Result<Self> {
        let (a, b) = (self.dim.as_dim(), other.dim.as_dim());
        let (ka, kb) = (self.kind.as_kind(), other.kind.as_kind());
        let kind = match sign {
            1 => ka.mul(a, kb, b),
            _ => ka.div(a, kb, b),
        }
        .into();
        let dim = match sign {
            1 => a.checked_mul(b),
            _ => a.checked_div(b),
        }
        .ok_or_else(|| anyhow!("dimension of {self} and {other} combined is out of range"))?
        .into();
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
            kind,
        })
    }

    fn checked_sqrt(self) -> Result<Self> {
        let dim = self
            .dim
            .as_dim()
            .checked_sqrt()
            .ok_or_else(|| anyhow!("square root of {self}, which already is a root"))?
            .into();
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
        let kind = self.kind.as_kind().sqrt().into();
        Ok(CanonicalUnit {
            powers,
            dim,
            magnitude,
            kind,
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
            match p.exp_x2 != 2 && p.symbol.contains(['·', '/', ' ']) {
                true => write!(f, "({})", p.symbol)?,
                false => f.write_str(&p.symbol)?,
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
/// user's `scale!`), `Pow10<E>`, `Pow2<E>`, `Rational<NUM, DEN>`, `FullScale<V, BITS>` or `FullScaleMax<V, BITS>`.
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
            let mut rest = unit.dim.clone();
            rest.rad -= 1;
            if !rest.as_dim().is_one() {
                bail!("Angle<{unit}>: {unit} is not a plane angle");
            }
            // a turn, 2π rad, in the unit
            let turn = Magnitude::ratio(2, 1)
                .checked_mul(Magnitude::PI)
                .and_then(|t| t.checked_div(unit.magnitude))
                .map_err(|e| anyhow!("a turn in {unit}: {e:?}"))?;
            let symbol = unit.to_string();
            Ok(OriginDefOwned {
                unit: UnitDefOwned {
                    symbol: symbol.clone(),
                    dim: unit.dim,
                    magnitude: unit.magnitude,
                },
                symbol,
                reference: None,
                offset: Offset::ZERO,
                modulus: Some(turn),
                step: Magnitude::ONE,
                shown: Offset::ZERO,
            })
        }
        _ => bail!("{} is not an origin", marker_name(s, bundle)?),
    }
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
        | MarkerDefOwned::ConventionalUnit(ConventionalUnitDefOwned { unit: def, .. })
            if def.symbol.is_empty()
                && def.dim.as_dim().is_one()
                && def.magnitude == Magnitude::ONE =>
        {
            Ok(CanonicalUnit::one())
        }
        MarkerDefOwned::Unit(def)
        | MarkerDefOwned::ConventionalUnit(ConventionalUnitDefOwned { unit: def, .. })
        | MarkerDefOwned::KindUnit(KindUnitDefOwned { unit: def, .. }) => Ok(CanonicalUnit {
            powers: vec![UnitPower {
                marker: marker_name(s, bundle)?,
                symbol: def.symbol.clone(),
                exp_x2: 2,
            }],
            dim: def.dim.clone(),
            magnitude: def.magnitude,
            kind: match definition(s, bundle)?.kind() {
                Some(k) => UnitKind::Named(k.to_string()),
                None => UnitKind::Plain,
            },
        }),
        MarkerDefOwned::Per => arg(0)?.checked_mul(arg(1)?, -1),
        MarkerDefOwned::Times => arg(0)?.checked_mul(arg(1)?, 1),
        MarkerDefOwned::Sqrt => arg(0)?.checked_sqrt(),
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
}

impl NumBounds {
    /// No bounds: `Unbounded`, `Wrapping`, `Saturating`.
    pub fn is_none(&self) -> bool {
        self.min.is_none() && self.max.is_none()
    }

    /// Whether `n` is within: an integer against integer bounds exactly, a float against float ones as
    /// `exact_units` compares them (NaN is out).
    pub fn contains(&self, n: &ww_numeric::NumericValue) -> bool {
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

/// The bounds marker `ty` is (the `B` of a `Fixed`, `Index` or `IndexFrom1`): `Unbounded`, `Wrapping` and
/// `Saturating` have none, `Range<MIN, MAX>` and `RangeU64<MIN, MAX>` integer ends, `RangeDec<MIN, MAX, EXP>` the
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
        MarkerDefOwned::Range | MarkerDefOwned::RangeU64 => NumBounds {
            min: Some(Bound::Int(int(0)?)),
            max: Some(Bound::Int(int(1)?)),
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

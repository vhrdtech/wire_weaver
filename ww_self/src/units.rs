//! Units and scales of a bundle, from the definitions it carries (INT-13): a unit type of an API (`Volt`,
//! `Per<Meter, Second>`, a user's `Rpm`) resolved to its dimension, its exact factor to coherent SI and a canonical
//! expression, the sorted product of the named units in it, so that a client knows the units of any crate without a
//! table of its own.
//!
//! A marker struct carries its [definition](crate::ItemStruct::marker): a named [unit](crate::MarkerDef::Unit) or
//! [scale](crate::MarkerDef::Scale), or one of `exact_units`' generic markers (`Per<A, B>`, `Pow10<E>`, ...) whose
//! arguments say the rest. [unit_of] and [scale_of] walk a type through them, in a bundle as received or
//! [prepared](crate::generics::prepare).
//!
//! The canonical expression is F#'s: `Per<Meter, Second>`, `Times<Per<Ratio, Second>, Meter>` and a user's own
//! `Per<Meter, Second>` re-declared elsewhere are the same `m·s⁻¹` when they name the same markers with the same
//! powers. Two units of the same [dimension](CanonicalUnit::dim) convert into each other by the quotient of their
//! [magnitudes](CanonicalUnit::magnitude) even when their expressions differ (`km/h` and `m/s`).

use crate::{
    ApiBundleOwned, GenericArgOwned, ItemStructOwned, MarkerDefOwned, TypeOwned, ValueOwned,
};
use anyhow::{Result, anyhow, bail};
use exact_units::{DimOwned, Magnitude};
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
}

/// The same unit: the same named units to the same powers. `km/h` and `m/s` are not the same, they are
/// [convertible](CanonicalUnit::factor_to).
impl PartialEq for CanonicalUnit {
    fn eq(&self, other: &Self) -> bool {
        self.powers.len() == other.powers.len()
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
        }
    }

    /// The exact factor a number in this unit is multiplied by to be in `other`, `None` when the dimensions
    /// differ: 1/1000 from `m` to `km`, π/30 from `rpm` to `rad/s`.
    pub fn factor_to(&self, other: &CanonicalUnit) -> Option<Magnitude> {
        if !self.dim.as_dim().same(other.dim.as_dim()) {
            return None;
        }
        self.magnitude.checked_div(other.magnitude).ok()
    }

    fn checked_mul(self, other: CanonicalUnit, sign: i16) -> Result<Self> {
        let (a, b) = (self.dim.as_dim(), other.dim.as_dim());
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
        Ok(CanonicalUnit {
            powers,
            dim,
            magnitude,
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

/// The unit `ty` is, through the [definitions](crate::ItemStruct::marker) in `bundle`: a named unit, or `Per`,
/// `Times`, `Sqrt` of units. An error for a type that is not a unit marker, or a marker without a definition (a crate
/// whose markers are hand-written, or a unit with an offset like °C, TODO(STD-74)).
pub fn unit_of(ty: &TypeOwned, bundle: &ApiBundleOwned) -> Result<CanonicalUnit> {
    unit_in(ty, bundle, 0)
}

/// The exact factor of the scale marker `ty` is (the `S` of a `Fixed<U, S, ..>`): a named scale (`prefix::Milli`, a
/// user's `scale!`), `Pow10<E>`, `Pow2<E>`, `Rational<NUM, DEN>`, `FullScale<V, BITS>` or `FullScaleMax<V, BITS>`.
pub fn scale_of(ty: &TypeOwned, bundle: &ApiBundleOwned) -> Result<Magnitude> {
    scale_in(ty, bundle, 0)
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
    s.marker.as_deref().ok_or_else(|| {
        anyhow!(
            "{} has no unit or scale definition",
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
        MarkerDefOwned::Unit(def) => Ok(CanonicalUnit {
            powers: vec![UnitPower {
                marker: marker_name(s, bundle)?,
                symbol: def.symbol.clone(),
                exp_x2: 2,
            }],
            dim: def.dim.clone(),
            magnitude: def.magnitude,
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

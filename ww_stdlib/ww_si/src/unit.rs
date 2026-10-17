//! What a unit is: its SI dimension, how its values become coherent SI, and how it is written.

use crate::SIExpr;
use crate::tree;
use core::fmt;
use shrink_wrap::prelude::*;

/// SI base-unit exponents of a unit: s, m, kg, A, K, mol, cd, each from -8 to 7. Volt is kg·m²·s⁻³·A⁻¹:
/// `Dim::new(-3, 2, 1, -1, 0, 0, 0)`.
///
/// On the wire seven 4-bit signed numbers in that order, 28 bits (`sized`): Volt is `d2 1f 00 0`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq, Hash),
    sized
)]
pub struct Dim {
    pub s: I4,
    pub m: I4,
    pub kg: I4,
    pub a: I4,
    pub k: I4,
    pub mol: I4,
    pub cd: I4,
}

/// An exponent in `const` code, a compile error (a panic at run time) when out of -8..=7.
pub(crate) const fn exp(e: i8) -> I4 {
    match I4::new(e) {
        Some(e) => e,
        None => panic!("SI exponent out of -8..=7"),
    }
}

const fn checked_exp(e: i16) -> Option<I4> {
    if e < i8::MIN as i16 || e > i8::MAX as i16 {
        return None;
    }
    I4::new(e as i8)
}

impl Dim {
    /// Dimensionless: a ratio, an angle in radians.
    pub const ONE: Dim = Dim::new(0, 0, 0, 0, 0, 0, 0);

    /// From the exponents of s, m, kg, A, K, mol, cd. Panics (a compile error in `const` code) when one is out of
    /// -8..=7, [checked_new](Self::checked_new) doesn't.
    pub const fn new(s: i8, m: i8, kg: i8, a: i8, k: i8, mol: i8, cd: i8) -> Dim {
        Dim {
            s: exp(s),
            m: exp(m),
            kg: exp(kg),
            a: exp(a),
            k: exp(k),
            mol: exp(mol),
            cd: exp(cd),
        }
    }

    /// From the exponents in the order of [new](Self::new), `None` when one is out of -8..=7.
    pub const fn checked_new(exps: [i8; 7]) -> Option<Dim> {
        let mut i = 0;
        while i < 7 {
            if I4::new(exps[i]).is_none() {
                return None;
            }
            i += 1;
        }
        Some(Dim::new(
            exps[0], exps[1], exps[2], exps[3], exps[4], exps[5], exps[6],
        ))
    }

    /// The exponents of s, m, kg, A, K, mol, cd.
    pub const fn exps(&self) -> [i8; 7] {
        [
            self.s.value(),
            self.m.value(),
            self.kg.value(),
            self.a.value(),
            self.k.value(),
            self.mol.value(),
            self.cd.value(),
        ]
    }

    const fn combine(self, o: Dim, sign: i16, times: i16) -> Option<Dim> {
        let a = self.exps();
        let b = o.exps();
        let mut out = [0i8; 7];
        let mut i = 0;
        while i < 7 {
            match checked_exp(a[i] as i16 * times + sign * b[i] as i16) {
                Some(e) => out[i] = e.value(),
                None => return None,
            }
            i += 1;
        }
        Dim::checked_new(out)
    }

    /// The dimension of a product: exponents added. `None` when one leaves -8..=7.
    pub const fn checked_mul(self, o: Dim) -> Option<Dim> {
        self.combine(o, 1, 1)
    }

    /// The dimension of a quotient: exponents subtracted. `None` when one leaves -8..=7.
    pub const fn checked_div(self, o: Dim) -> Option<Dim> {
        self.combine(o, -1, 1)
    }

    /// The dimension of a power: exponents multiplied by `n`. `None` when one leaves -8..=7.
    pub const fn checked_pow(self, n: i8) -> Option<Dim> {
        self.combine(Dim::ONE, 0, n as i16)
    }

    /// The dimension of a product, for unit markers: panics (a compile error in `const` code) when an exponent leaves
    /// -8..=7.
    pub const fn mul(self, o: Dim) -> Dim {
        match self.checked_mul(o) {
            Some(d) => d,
            None => panic!("SI exponent out of -8..=7"),
        }
    }

    /// The dimension of a quotient, for unit markers: panics (a compile error in `const` code) when an exponent
    /// leaves -8..=7.
    pub const fn div(self, o: Dim) -> Dim {
        match self.checked_div(o) {
            Some(d) => d,
            None => panic!("SI exponent out of -8..=7"),
        }
    }

    /// `==` in `const` code.
    pub const fn same(self, o: Dim) -> bool {
        let a = self.exps();
        let b = o.exps();
        let mut i = 0;
        while i < 7 {
            if a[i] != b[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// Written the SI way, base units in the order kg, m, s, A, K, mol, cd: `kg·m²·s⁻³·A⁻¹`, `1` when dimensionless.
impl fmt::Display for Dim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts = [
            ("kg", self.kg.value()),
            ("m", self.m.value()),
            ("s", self.s.value()),
            ("A", self.a.value()),
            ("K", self.k.value()),
            ("mol", self.mol.value()),
            ("cd", self.cd.value()),
        ];
        let mut first = true;
        for (symbol, exp) in parts {
            if exp == 0 {
                continue;
            }
            if !first {
                f.write_str("·")?;
            }
            first = false;
            f.write_str(symbol)?;
            if exp != 1 {
                write_superscript(f, exp)?;
            }
        }
        if first {
            f.write_str("1")?;
        }
        Ok(())
    }
}

pub(crate) fn write_superscript(f: &mut fmt::Formatter<'_>, exp: i8) -> fmt::Result {
    const DIGITS: [&str; 10] = ["⁰", "¹", "²", "³", "⁴", "⁵", "⁶", "⁷", "⁸", "⁹"];
    if exp < 0 {
        f.write_str("⁻")?;
    }
    let exp = exp.unsigned_abs();
    if exp >= 100 {
        f.write_str(DIGITS[(exp / 100) as usize])?;
    }
    if exp >= 10 {
        f.write_str(DIGITS[(exp / 10 % 10) as usize])?;
    }
    f.write_str(DIGITS[(exp % 10) as usize])
}

/// How a value in a unit becomes coherent SI (the unit of [Dim] with no factor): `si = value · factor + offset`.
///
/// On the wire a nibble for the variant and its numbers: `Coherent` is `0`, `Pow10(-3)` is `1` and `fd`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq),
    ww_repr = u4,
    // part of a unit's description: a reader can't convert with a scale it doesn't know
    no_future_version
)]
pub enum Scale {
    /// The unit is coherent SI: V, A, N·m, rad.
    Coherent,
    /// A power of ten: g is 10⁻³ kg, L is 10⁻³ m³.
    Pow10(i8),
    /// Any factor: ° is π/180 rad, eV is 1.602176634·10⁻¹⁹ J.
    Factor(f64),
    /// A factor and an offset: °C is K − 273.15, `Affine { factor: 1.0, offset: 273.15 }`.
    Affine { factor: f64, offset: f64 },
}

impl Scale {
    /// The factor, without the offset of an `Affine` scale.
    pub const fn factor(&self) -> f64 {
        match *self {
            Scale::Coherent => 1.0,
            Scale::Pow10(exp) => crate::ops::pow10_f64(exp),
            Scale::Factor(factor) => factor,
            Scale::Affine { factor, .. } => factor,
        }
    }

    /// The offset of an `Affine` scale, `0.0` for the others.
    pub const fn offset(&self) -> f64 {
        match *self {
            Scale::Affine { offset, .. } => offset,
            _ => 0.0,
        }
    }
}

/// A unit as a tree: named units, and products and quotients of them (`Per<Torque, AngularVelocity>` is
/// `Div(Torque, AngularVelocity)`). Every [Unit] has one in [Unit::EXPR]; [units::by_name](crate::units::by_name)
/// finds the named ones of this crate.
#[derive(Debug, PartialEq)]
pub enum UnitExpr {
    Named {
        /// The marker's name: `"Volt"`.
        name: &'static str,
        /// `"V"`, empty for [Ratio](crate::units::Ratio).
        symbol: &'static str,
        dim: Dim,
        scale: Scale,
    },
    Mul(&'static UnitExpr, &'static UnitExpr),
    Div(&'static UnitExpr, &'static UnitExpr),
    /// A unit to an integer power: m³ as `Pow(Meter, 3)`. No marker of this crate is one, a user's marker can be.
    Pow(&'static UnitExpr, i8),
}

impl UnitExpr {
    /// The SI dimension of the whole expression.
    pub const fn dim(&self) -> Dim {
        match self {
            UnitExpr::Named { dim, .. } => *dim,
            UnitExpr::Mul(a, b) => a.dim().mul(b.dim()),
            UnitExpr::Div(a, b) => a.dim().div(b.dim()),
            UnitExpr::Pow(a, n) => match a.dim().checked_pow(*n) {
                Some(d) => d,
                None => panic!("SI exponent out of -8..=7"),
            },
        }
    }

    /// The factor to coherent SI of the whole expression (offsets left out: a product with °C is not affine).
    pub const fn factor(&self) -> f64 {
        match self {
            UnitExpr::Named { scale, .. } => scale.factor(),
            UnitExpr::Mul(a, b) => a.factor() * b.factor(),
            UnitExpr::Div(a, b) => a.factor() / b.factor(),
            UnitExpr::Pow(a, n) => crate::ops::powi_f64(a.factor(), *n),
        }
    }

    /// The offset to coherent SI: only a named unit with an `Affine` scale has one.
    pub const fn offset(&self) -> f64 {
        match self {
            UnitExpr::Named { scale, .. } => scale.offset(),
            _ => 0.0,
        }
    }

    /// Whether a named unit in the expression has an `Affine` scale (°C).
    pub const fn has_offset(&self) -> bool {
        match self {
            UnitExpr::Named { scale, .. } => matches!(scale, Scale::Affine { .. }),
            UnitExpr::Mul(a, b) | UnitExpr::Div(a, b) => a.has_offset() || b.has_offset(),
            UnitExpr::Pow(a, _) => a.has_offset(),
        }
    }

    /// The symbol as written after a number: `V`, `N·m/(rad/s)`, empty for a ratio. Same as `Display`.
    pub fn symbol(&self) -> impl fmt::Display + '_ {
        self
    }

    /// The type name: `Volt`, `Per<Torque, AngularVelocity>`.
    pub fn name(&self) -> impl fmt::Display + '_ {
        tree::name_of(self)
    }
}

/// The symbol: `V`, `N·m/(rad/s)`, `(rad/s)·(rad/s)`.
impl fmt::Display for UnitExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&tree::symbol(&self), f)
    }
}

/// A unit marker: a zero-size type that is never sent, only named in types (`Quantity<units::Volt>`). The markers of
/// this crate are in [units](crate::units); an API crate can declare its own the same way.
///
/// ```
/// use ww_si::{Dim, Quantity, Scale, Unit, UnitExpr};
///
/// /// Revolutions per minute.
/// #[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
/// pub struct Rpm;
///
/// impl Unit for Rpm {
///     const DIM: Dim = Dim::new(-1, 0, 0, 0, 0, 0, 0);
///     const EXPR: &'static UnitExpr = &UnitExpr::Named {
///         name: "Rpm",
///         symbol: "rpm",
///         dim: Self::DIM,
///         scale: Scale::Factor(core::f64::consts::TAU / 60.0),
///     };
/// }
///
/// let speed = Quantity::<Rpm>::base(60u8);
/// assert_eq!(format!("{speed}"), "60 rpm");
/// assert!((speed.to_si_f64() - core::f64::consts::TAU).abs() < 1e-12); // rad/s
/// ```
pub trait Unit: Copy + Default + 'static {
    /// SI base-unit exponents.
    const DIM: Dim;
    /// Name, symbol and scale, or the tree of a compound unit.
    const EXPR: &'static UnitExpr;
    /// The same unit as it goes on the wire in an [AnyQuantity](crate::AnyQuantity). Made from [EXPR](Self::EXPR)
    /// for a named unit; a compound marker of its own (not `Per` / `Times`) writes its tree here too.
    const SI_EXPR: &'static SIExpr<'static> = &SIExpr::named(Self::EXPR);
}

/// `Self · R` is a `Self::Output`: filled in by this crate's product table (`Volt · Ampere = Watt`), and for any unit
/// times a [Ratio](crate::units::Ratio).
pub trait UnitMul<R: Unit>: Unit {
    type Output: Unit;
}

/// `Self / R` is a `Self::Output`: filled in by this crate's product table (`Watt / Volt = Ampere`), and for any unit
/// divided by a [Ratio](crate::units::Ratio).
pub trait UnitDiv<R: Unit>: Unit {
    type Output: Unit;
}

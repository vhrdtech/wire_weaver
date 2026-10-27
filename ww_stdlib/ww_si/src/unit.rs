//! What a unit is: its dimension, how its values become coherent SI, and how it is written.

use crate::SIExpr;
use crate::tree;
use core::fmt;
use shrink_wrap::prelude::*;

/// Most counted bases a [Dim] has at once (`Sample·Frame⁻¹` has two).
pub const MAX_COUNTED: usize = 4;

/// A counted base of a [Dim] and its exponent: what is counted (`"Sample"`, `"Block"`, an API's own marker name) and
/// how often it is in the dimension (`Sample/s` has `Sample` to the 1st power).
///
/// A base is told apart by its name only, so two APIs' own `Block` markers are the same base at run time (in types
/// they stay different markers).
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug, PartialEq, Eq, Hash),
    derive_borrowed(Copy),
    final_structure
)]
pub struct Counted<'i> {
    pub name: &'i str,
    pub exp: I4,
}

/// The dimension of a unit: exponents of the 7 SI base units s, m, kg, A, K, mol, cd, of angle (rad) and of
/// information (bit), each from -8 to 7, and of up to [MAX_COUNTED] counted bases (`Sample`, `Block`, `Item`, ..).
/// Volt is kg·m²·s⁻³·A⁻¹: `Dim::new(-3, 2, 1, -1, 0, 0, 0)`; rad/s is `Dim::new(-1, 0, 0, 0, 0, 0, 0).with_rad(1)`;
/// samples per second is `Dim::counted("Sample").div(Dim::new(1, 0, 0, 0, 0, 0, 0))`.
///
/// Values convert into each other and add only with the same dimension: bytes never become a ratio, an angle never
/// a frequency, samples never bytes (that takes a `Per<Byte, Sample>`).
///
/// On the wire nine 4-bit signed numbers in the order above, then for each of the [MAX_COUNTED] slots a bit for
/// whether it is used, and a used one's name and exponent. Used slots come first, sorted by name, with exponents that
/// aren't 0 (a value that isn't so doesn't decode), so two equal dimensions have the same bytes: Volt is
/// `d2 1f 00 00 00` (40 bits).
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug, PartialEq, Eq, Hash),
    derive_borrowed(Copy),
    final_structure,
    validate = Self::validate
)]
pub struct Dim<'i> {
    pub s: I4,
    pub m: I4,
    pub kg: I4,
    pub a: I4,
    pub k: I4,
    pub mol: I4,
    pub cd: I4,
    /// Plane angle, radian.
    pub rad: I4,
    /// Information, bit.
    pub bit: I4,
    /// Counted bases, see [Counted]: used slots first, sorted by name.
    pub counted: [Option<Counted<'i>>; 4],
}

const _: () = assert!(MAX_COUNTED == 4, "the array length in Dim");

/// Number of fixed exponents: s, m, kg, A, K, mol, cd, rad, bit.
const FIXED: usize = 9;

/// An exponent in `const` code, a compile error (a panic at run time) when out of -8..=7.
pub(crate) const fn exp(e: i8) -> I4 {
    match I4::new(e) {
        Some(e) => e,
        None => panic!("exponent of a dimension out of -8..=7"),
    }
}

const fn checked_exp(e: i16) -> Option<I4> {
    if e < i8::MIN as i16 || e > i8::MAX as i16 {
        return None;
    }
    I4::new(e as i8)
}

/// Byte-wise order of two names, in `const` code.
const fn cmp_names(a: &str, b: &str) -> core::cmp::Ordering {
    use core::cmp::Ordering;
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut i = 0;
    while i < a.len() && i < b.len() {
        if a[i] != b[i] {
            return if a[i] < b[i] {
                Ordering::Less
            } else {
                Ordering::Greater
            };
        }
        i += 1;
    }
    if a.len() < b.len() {
        Ordering::Less
    } else if a.len() > b.len() {
        Ordering::Greater
    } else {
        Ordering::Equal
    }
}

const fn same_name(a: &str, b: &str) -> bool {
    matches!(cmp_names(a, b), core::cmp::Ordering::Equal)
}

impl Dim<'static> {
    /// Dimensionless: a ratio.
    pub const ONE: Dim<'static> = Dim::new(0, 0, 0, 0, 0, 0, 0);
}

impl<'i> Dim<'i> {
    /// From the exponents of s, m, kg, A, K, mol, cd, without angle, information or counted bases (add them with
    /// [with_rad](Self::with_rad), [with_bit](Self::with_bit), [mul](Self::mul) by [counted](Self::counted)). Panics
    /// (a compile error in `const` code) when one is out of -8..=7, [checked_new](Self::checked_new) doesn't.
    pub const fn new(s: i8, m: i8, kg: i8, a: i8, k: i8, mol: i8, cd: i8) -> Dim<'i> {
        Dim {
            s: exp(s),
            m: exp(m),
            kg: exp(kg),
            a: exp(a),
            k: exp(k),
            mol: exp(mol),
            cd: exp(cd),
            rad: exp(0),
            bit: exp(0),
            counted: [None; MAX_COUNTED],
        }
    }

    /// From the exponents of s, m, kg, A, K, mol, cd, rad, bit, `None` when one is out of -8..=7.
    pub const fn checked_new(exps: [i8; FIXED]) -> Option<Dim<'i>> {
        let mut i = 0;
        while i < FIXED {
            if I4::new(exps[i]).is_none() {
                return None;
            }
            i += 1;
        }
        Some(Dim::from_fixed(exps))
    }

    const fn from_fixed(e: [i8; FIXED]) -> Dim<'i> {
        Dim::new(e[0], e[1], e[2], e[3], e[4], e[5], e[6])
            .with_rad(e[7])
            .with_bit(e[8])
    }

    /// A count of `name` to the 1st power: the dimension of a counted unit marker (`Dim::counted("Sample")`).
    pub const fn counted(name: &'i str) -> Dim<'i> {
        let mut d = Dim::new(0, 0, 0, 0, 0, 0, 0);
        d.counted[0] = Some(Counted { name, exp: exp(1) });
        d
    }

    /// The same with the exponent of angle (rad) set to `e`.
    pub const fn with_rad(mut self, e: i8) -> Dim<'i> {
        self.rad = exp(e);
        self
    }

    /// The same with the exponent of information (bit) set to `e`.
    pub const fn with_bit(mut self, e: i8) -> Dim<'i> {
        self.bit = exp(e);
        self
    }

    /// The same without angle, as SI writes it (rad = 1): `Torque` (kg·m²·s⁻²·rad⁻¹) becomes the joule's
    /// kg·m²·s⁻², `AngularVelocity` s⁻¹. The explicit bridge for exports to systems without an angle dimension
    /// (UCUM, UNECE, Pint, uom) and for formulas like v = ωr; never applied implicitly.
    pub const fn strip_radian(self) -> Dim<'i> {
        self.with_rad(0)
    }

    /// The exponents of s, m, kg, A, K, mol, cd, rad, bit.
    pub const fn exps(&self) -> [i8; FIXED] {
        [
            self.s.value(),
            self.m.value(),
            self.kg.value(),
            self.a.value(),
            self.k.value(),
            self.mol.value(),
            self.cd.value(),
            self.rad.value(),
            self.bit.value(),
        ]
    }

    /// The counted bases and their exponents, sorted by name.
    pub fn counted_bases(&self) -> impl Iterator<Item = (&'i str, i8)> + '_ {
        self.counted
            .iter()
            .map_while(|c| c.map(|c| (c.name, c.exp.value())))
    }

    /// Without any exponent: a ratio.
    pub const fn is_one(&self) -> bool {
        self.same(Dim::ONE)
    }

    /// `self^times · o^sign`, `None` when an exponent leaves -8..=7 or more than [MAX_COUNTED] counted bases are left.
    const fn combine(self, o: Dim<'i>, sign: i16, times: i16) -> Option<Dim<'i>> {
        let a = self.exps();
        let b = o.exps();
        let mut fixed = [0i8; FIXED];
        let mut i = 0;
        while i < FIXED {
            match checked_exp(a[i] as i16 * times + sign * b[i] as i16) {
                Some(e) => fixed[i] = e.value(),
                None => return None,
            }
            i += 1;
        }
        let mut out = Dim::from_fixed(fixed);
        // both lists are sorted: merge them, adding the exponents of a base in both
        let (mut i, mut j, mut n) = (0, 0, 0);
        loop {
            let (name, e) = match (self.slot(i), o.slot(j)) {
                (None, None) => break,
                (Some((na, ea)), None) => {
                    i += 1;
                    (na, ea as i16 * times)
                }
                (None, Some((nb, eb))) => {
                    j += 1;
                    (nb, sign * eb as i16)
                }
                (Some((na, ea)), Some((nb, eb))) => match cmp_names(na, nb) {
                    core::cmp::Ordering::Less => {
                        i += 1;
                        (na, ea as i16 * times)
                    }
                    core::cmp::Ordering::Greater => {
                        j += 1;
                        (nb, sign * eb as i16)
                    }
                    core::cmp::Ordering::Equal => {
                        i += 1;
                        j += 1;
                        (na, ea as i16 * times + sign * eb as i16)
                    }
                },
            };
            if e == 0 {
                continue;
            }
            let Some(exp) = checked_exp(e) else {
                return None;
            };
            if n == MAX_COUNTED {
                return None;
            }
            out.counted[n] = Some(Counted { name, exp });
            n += 1;
        }
        Some(out)
    }

    const fn slot(&self, i: usize) -> Option<(&'i str, i8)> {
        if i >= MAX_COUNTED {
            return None;
        }
        match self.counted[i] {
            Some(c) => Some((c.name, c.exp.value())),
            None => None,
        }
    }

    /// The dimension of a product: exponents added. `None` when one leaves -8..=7 or there are more than
    /// [MAX_COUNTED] counted bases.
    pub const fn checked_mul(self, o: Dim<'i>) -> Option<Dim<'i>> {
        self.combine(o, 1, 1)
    }

    /// The dimension of a quotient: exponents subtracted. `None` like [checked_mul](Self::checked_mul).
    pub const fn checked_div(self, o: Dim<'i>) -> Option<Dim<'i>> {
        self.combine(o, -1, 1)
    }

    /// The dimension of a power: exponents multiplied by `n`. `None` when one leaves -8..=7.
    pub const fn checked_pow(self, n: i8) -> Option<Dim<'i>> {
        self.combine(Dim::new(0, 0, 0, 0, 0, 0, 0), 0, n as i16)
    }

    /// The dimension of a product, for unit markers: panics (a compile error in `const` code) when an exponent leaves
    /// -8..=7 or there are more than [MAX_COUNTED] counted bases.
    pub const fn mul(self, o: Dim<'i>) -> Dim<'i> {
        match self.checked_mul(o) {
            Some(d) => d,
            None => panic!("exponent of a dimension out of -8..=7, or too many counted bases"),
        }
    }

    /// The dimension of a quotient, for unit markers: panics like [mul](Self::mul).
    pub const fn div(self, o: Dim<'i>) -> Dim<'i> {
        match self.checked_div(o) {
            Some(d) => d,
            None => panic!("exponent of a dimension out of -8..=7, or too many counted bases"),
        }
    }

    /// `==` in `const` code.
    pub const fn same(self, o: Dim<'_>) -> bool {
        let a = self.exps();
        let b = o.exps();
        let mut i = 0;
        while i < FIXED {
            if a[i] != b[i] {
                return false;
            }
            i += 1;
        }
        let mut i = 0;
        while i < MAX_COUNTED {
            match (self.counted[i], o.counted[i]) {
                (None, None) => {}
                (Some(x), Some(y)) => {
                    if !same_name(x.name, y.name) || x.exp.value() != y.exp.value() {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// Whether the counted bases are as they must be on the wire: used slots first, sorted by name without
    /// duplicates, exponents not 0.
    pub const fn is_canonical(&self) -> bool {
        let mut i = 0;
        let mut ended = false;
        while i < MAX_COUNTED {
            match self.counted[i] {
                None => ended = true,
                Some(c) => {
                    if ended || c.exp.value() == 0 {
                        return false;
                    }
                    if i > 0 {
                        if let Some(prev) = self.counted[i - 1] {
                            if !matches!(cmp_names(prev.name, c.name), core::cmp::Ordering::Less) {
                                return false;
                            }
                        }
                    }
                }
            }
            i += 1;
        }
        true
    }

    fn validate(&self) -> Result<(), ShrinkWrapError> {
        if self.is_canonical() {
            Ok(())
        } else {
            Err(ShrinkWrapError::SubtypeOutOfRange)
        }
    }
}

/// Written the SI way, base units in the order kg, m, s, A, K, mol, cd, then rad, bit and the counted bases by name:
/// `kg·m²·s⁻³·A⁻¹`, `rad·s⁻¹`, `Sample·s⁻¹`, `1` when dimensionless.
impl fmt::Display for Dim<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts = [
            ("kg", self.kg.value()),
            ("m", self.m.value()),
            ("s", self.s.value()),
            ("A", self.a.value()),
            ("K", self.k.value()),
            ("mol", self.mol.value()),
            ("cd", self.cd.value()),
            ("rad", self.rad.value()),
            ("bit", self.bit.value()),
        ];
        let mut first = true;
        for (symbol, exp) in parts.into_iter().chain(self.counted_bases()) {
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

#[cfg(feature = "std")]
impl DimOwned {
    fn validate(&self) -> Result<(), ShrinkWrapError> {
        self.as_dim().validate()
    }

    /// The same dimension, borrowing the names of counted bases.
    pub fn as_dim(&self) -> Dim<'_> {
        let mut counted = [None; MAX_COUNTED];
        for (slot, c) in counted.iter_mut().zip(&self.counted) {
            *slot = c.as_ref().map(|c| Counted {
                name: c.name.as_str(),
                exp: c.exp,
            });
        }
        Dim {
            s: self.s,
            m: self.m,
            kg: self.kg,
            a: self.a,
            k: self.k,
            mol: self.mol,
            cd: self.cd,
            rad: self.rad,
            bit: self.bit,
            counted,
        }
    }
}

#[cfg(feature = "std")]
impl From<Dim<'_>> for DimOwned {
    fn from(d: Dim<'_>) -> Self {
        DimOwned {
            s: d.s,
            m: d.m,
            kg: d.kg,
            a: d.a,
            k: d.k,
            mol: d.mol,
            cd: d.cd,
            rad: d.rad,
            bit: d.bit,
            counted: d.counted.map(|c| {
                c.map(|c| CountedOwned {
                    name: c.name.into(),
                    exp: c.exp,
                })
            }),
        }
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
        dim: Dim<'static>,
        scale: Scale,
    },
    Mul(&'static UnitExpr, &'static UnitExpr),
    Div(&'static UnitExpr, &'static UnitExpr),
    /// A unit to an integer power: m³ as `Pow(Meter, 3)`. No marker of this crate is one, a user's marker can be.
    Pow(&'static UnitExpr, i8),
}

impl UnitExpr {
    /// The dimension of the whole expression.
    pub const fn dim(&self) -> Dim<'static> {
        match self {
            UnitExpr::Named { dim, .. } => *dim,
            UnitExpr::Mul(a, b) => a.dim().mul(b.dim()),
            UnitExpr::Div(a, b) => a.dim().div(b.dim()),
            UnitExpr::Pow(a, n) => match a.dim().checked_pow(*n) {
                Some(d) => d,
                None => panic!("exponent of a dimension out of -8..=7"),
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
///     // rad/s
///     const DIM: Dim<'static> = Dim::new(-1, 0, 0, 0, 0, 0, 0).with_rad(1);
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
    /// The dimension: SI base-unit exponents, angle, information and counted bases.
    const DIM: Dim<'static>;
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

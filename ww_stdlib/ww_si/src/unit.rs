//! What a unit is: its SI dimension, how its values become coherent SI, and how it is written.

use core::fmt;

/// SI base-unit exponents of a unit: s, m, kg, A, K, mol, cd. Volt is kg·m²·s⁻³·A⁻¹:
/// `Dim { s: -3, m: 2, kg: 1, a: -1, ..Dim::ONE }`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Dim {
    pub s: i8,
    pub m: i8,
    pub kg: i8,
    pub a: i8,
    pub k: i8,
    pub mol: i8,
    pub cd: i8,
}

impl Dim {
    /// Dimensionless: a ratio, an angle in radians.
    pub const ONE: Dim = Dim {
        s: 0,
        m: 0,
        kg: 0,
        a: 0,
        k: 0,
        mol: 0,
        cd: 0,
    };

    /// The dimension of a product: exponents added.
    pub const fn mul(self, o: Dim) -> Dim {
        Dim {
            s: self.s + o.s,
            m: self.m + o.m,
            kg: self.kg + o.kg,
            a: self.a + o.a,
            k: self.k + o.k,
            mol: self.mol + o.mol,
            cd: self.cd + o.cd,
        }
    }

    /// The dimension of a quotient: exponents subtracted.
    pub const fn div(self, o: Dim) -> Dim {
        Dim {
            s: self.s - o.s,
            m: self.m - o.m,
            kg: self.kg - o.kg,
            a: self.a - o.a,
            k: self.k - o.k,
            mol: self.mol - o.mol,
            cd: self.cd - o.cd,
        }
    }

    /// `==` in `const` code.
    pub const fn same(self, o: Dim) -> bool {
        self.s == o.s
            && self.m == o.m
            && self.kg == o.kg
            && self.a == o.a
            && self.k == o.k
            && self.mol == o.mol
            && self.cd == o.cd
    }
}

/// Written the SI way, base units in the order kg, m, s, A, K, mol, cd: `kg·m²·s⁻³·A⁻¹`, `1` when dimensionless.
impl fmt::Display for Dim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts = [
            ("kg", self.kg),
            ("m", self.m),
            ("s", self.s),
            ("A", self.a),
            ("K", self.k),
            ("mol", self.mol),
            ("cd", self.cd),
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

fn write_superscript(f: &mut fmt::Formatter<'_>, exp: i8) -> fmt::Result {
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
#[derive(Copy, Clone, Debug, PartialEq)]
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
}

impl UnitExpr {
    /// The SI dimension of the whole expression.
    pub const fn dim(&self) -> Dim {
        match self {
            UnitExpr::Named { dim, .. } => *dim,
            UnitExpr::Mul(a, b) => a.dim().mul(b.dim()),
            UnitExpr::Div(a, b) => a.dim().div(b.dim()),
        }
    }

    /// The factor to coherent SI of the whole expression (offsets left out: a product with °C is not affine).
    pub const fn factor(&self) -> f64 {
        match self {
            UnitExpr::Named { scale, .. } => scale.factor(),
            UnitExpr::Mul(a, b) => a.factor() * b.factor(),
            UnitExpr::Div(a, b) => a.factor() / b.factor(),
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
        }
    }

    /// The symbol as written after a number: `V`, `N·m/(rad/s)`, empty for a ratio. Same as `Display`.
    pub fn symbol(&self) -> impl fmt::Display + '_ {
        self
    }

    /// The type name: `Volt`, `Per<Torque, AngularVelocity>`.
    pub fn name(&self) -> impl fmt::Display + '_ {
        UnitName(self)
    }

    /// Needs parentheses when it stands next to `·` or `/`.
    fn is_compound(&self) -> bool {
        match self {
            UnitExpr::Named { symbol, .. } => symbol.contains(['/', '·']),
            _ => true,
        }
    }
}

/// The symbol: `V`, `N·m/(rad/s)`, `(rad/s)·(rad/s)`.
impl fmt::Display for UnitExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fn side(f: &mut fmt::Formatter<'_>, e: &UnitExpr, parens: bool) -> fmt::Result {
            if parens {
                write!(f, "({e})")
            } else {
                write!(f, "{e}")
            }
        }
        match self {
            UnitExpr::Named { symbol, .. } => f.write_str(symbol),
            UnitExpr::Mul(a, b) => {
                side(f, a, a.is_compound() && !matches!(a, UnitExpr::Mul(..)))?;
                f.write_str("·")?;
                side(f, b, b.is_compound() && !matches!(b, UnitExpr::Mul(..)))
            }
            UnitExpr::Div(a, b) => {
                side(f, a, matches!(a, UnitExpr::Div(..)))?;
                f.write_str("/")?;
                side(f, b, b.is_compound())
            }
        }
    }
}

struct UnitName<'a>(&'a UnitExpr);

impl fmt::Display for UnitName<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            UnitExpr::Named { name, .. } => f.write_str(name),
            UnitExpr::Mul(a, b) => write!(f, "Times<{}, {}>", a.name(), b.name()),
            UnitExpr::Div(a, b) => write!(f, "Per<{}, {}>", a.name(), b.name()),
        }
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
///     const DIM: Dim = Dim { s: -1, ..Dim::ONE };
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

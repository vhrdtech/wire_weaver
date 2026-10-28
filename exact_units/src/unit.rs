//! What a unit is: its dimension, how its values become coherent SI, and how it is written.

#[cfg(feature = "shrink_wrap")]
use crate::SIExpr;
use crate::tree;
use core::fmt;
#[cfg(feature = "shrink_wrap")]
use shrink_wrap::prelude::*;

/// Most counted bases a [Dim] has at once (`Sample·Frame⁻¹` has two).
pub const MAX_COUNTED: usize = 4;

/// A counted base of a [Dim] and its exponent: what is counted, named by the crate and the marker that define it
/// (`exact_units` `Sample`, an API crate's own `Bucket`), and how often it is in the dimension (`Sample/s` has `Sample` to
/// the 1st power).
///
/// A base is told apart by crate and marker name together: two crates' `Sample` markers are different bases, and a
/// marker stays the same base when it moves between modules of its crate. Shown by the marker name alone.
#[cfg_attr(
    feature = "shrink_wrap",
    derive_shrink_wrap(
        borrowed,
        owned(feature = "std"),
        derive_owned(Clone, Debug, PartialEq, Eq, Hash),
        cfg_attr_owned(feature = "serde", derive(serde::Serialize, serde::Deserialize)),
        final_structure
    )
)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Counted<'i> {
    /// The crate that defines the marker, as Rust names it (`exact_units`, `my_api`): `CARGO_CRATE_NAME`.
    pub crate_name: &'i str,
    /// The marker's name: `Sample`.
    pub name: &'i str,
    #[cfg_attr(feature = "shrink_wrap", wire(I4))]
    pub exp: i8,
}

/// The dimension of a unit: exponents of the 7 SI base units s, m, kg, A, K, mol, cd, of angle (rad) and of
/// information (bit), each from -8 to 7, and of up to [MAX_COUNTED] counted bases (`Sample`, `Block`, `Item`, ..).
/// Volt is kg·m²·s⁻³·A⁻¹: `Dim::new(-3, 2, 1, -1, 0, 0, 0)`; rad/s is `Dim::new(-1, 0, 0, 0, 0, 0, 0).with_rad(1)`;
/// samples per second is `Dim::counted("exact_units", "Sample").div(Dim::new(1, 0, 0, 0, 0, 0, 0))`.
///
/// With `root` set, the dimension is the square root of the one the exponents describe: a noise density in V/√Hz
/// is kg²·m⁴·s⁻⁵·A⁻² with `root` (`Volt` squared over hertz), shown as `kg·m²·s⁻⁵ᐟ²·A⁻¹`. Only half-integer
/// powers are written this way, so `root` is set only when some exponent is odd ([sqrt](Self::sqrt) and
/// [mul](Self::mul) keep it so).
///
/// Values convert into each other and add only with the same dimension: bytes never become a ratio, an angle never
/// a frequency, samples never bytes (that takes a `Per<Byte, Sample>`).
///
/// On the wire nine 4-bit signed numbers in the order above, the `root` bit, then for each of the [MAX_COUNTED]
/// slots a bit for whether it is used, and a used one's crate name, name and exponent. Used slots come first, sorted
/// by crate and name, with exponents that aren't 0, and `root` only with an odd exponent (a value that isn't so
/// doesn't decode), so two equal dimensions have the same bytes: Volt is `d2 1f 00 00 00 00` (41 bits).
#[cfg_attr(
    feature = "shrink_wrap",
    derive_shrink_wrap(
        borrowed,
        owned(feature = "std"),
        derive_owned(Clone, Debug, PartialEq, Eq, Hash),
        cfg_attr_owned(feature = "serde", derive(serde::Serialize, serde::Deserialize)),
        final_structure,
        validate = Self::validate
    )
)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Dim<'i> {
    #[cfg_attr(feature = "shrink_wrap", wire(I4))]
    pub s: i8,
    #[cfg_attr(feature = "shrink_wrap", wire(I4))]
    pub m: i8,
    #[cfg_attr(feature = "shrink_wrap", wire(I4))]
    pub kg: i8,
    #[cfg_attr(feature = "shrink_wrap", wire(I4))]
    pub a: i8,
    #[cfg_attr(feature = "shrink_wrap", wire(I4))]
    pub k: i8,
    #[cfg_attr(feature = "shrink_wrap", wire(I4))]
    pub mol: i8,
    #[cfg_attr(feature = "shrink_wrap", wire(I4))]
    pub cd: i8,
    /// Plane angle, radian.
    #[cfg_attr(feature = "shrink_wrap", wire(I4))]
    pub rad: i8,
    /// Information, bit.
    #[cfg_attr(feature = "shrink_wrap", wire(I4))]
    pub bit: i8,
    /// The square root of what the exponents describe, see above.
    pub root: bool,
    /// Counted bases, see [Counted]: used slots first, sorted by crate and name.
    pub counted: [Option<Counted<'i>>; 4],
}

const _: () = assert!(MAX_COUNTED == 4, "the array length in Dim");

/// Number of fixed exponents: s, m, kg, A, K, mol, cd, rad, bit.
const FIXED: usize = 9;

/// An exponent in `const` code, a compile error (a panic at run time) when out of -8..=7.
pub const fn exp(e: i8) -> i8 {
    if !in_range(e) {
        panic!("exponent of a dimension out of -8..=7");
    }
    e
}

/// An exponent a [Dim] holds: -8..=7, a 4-bit signed number on the wire.
const fn in_range(e: i8) -> bool {
    e >= -8 && e <= 7
}

const fn checked_exp(e: i16) -> Option<i8> {
    if e < i8::MIN as i16 || e > i8::MAX as i16 {
        return None;
    }
    if in_range(e as i8) {
        Some(e as i8)
    } else {
        None
    }
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

/// Order of two counted bases: by crate, then by marker name.
const fn cmp_bases(a: (&str, &str), b: (&str, &str)) -> core::cmp::Ordering {
    match cmp_names(a.0, b.0) {
        core::cmp::Ordering::Equal => cmp_names(a.1, b.1),
        o => o,
    }
}

/// Most counted bases while two dimensions are combined, before zero exponents are dropped.
const MERGED: usize = 2 * MAX_COUNTED;

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
            root: false,
            counted: [None; MAX_COUNTED],
        }
    }

    /// From the exponents of s, m, kg, A, K, mol, cd, rad, bit, `None` when one is out of -8..=7.
    pub const fn checked_new(exps: [i8; FIXED]) -> Option<Dim<'i>> {
        let mut i = 0;
        while i < FIXED {
            if !in_range(exps[i]) {
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

    /// A count of the marker `name` of `crate_name` to the 1st power: the dimension of a counted unit marker
    /// (`Dim::counted("exact_units", "Sample")`). In an API crate, [counted!](crate::counted) fills in the crate.
    pub const fn counted(crate_name: &'i str, name: &'i str) -> Dim<'i> {
        let mut d = Dim::new(0, 0, 0, 0, 0, 0, 0);
        d.counted[0] = Some(Counted {
            crate_name,
            name,
            exp: exp(1),
        });
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
            self.s, self.m, self.kg, self.a, self.k, self.mol, self.cd, self.rad, self.bit,
        ]
    }

    /// The marker names of the counted bases and their exponents, in the order of the wire (by crate and name), as
    /// they are shown; [counted](Self::counted) has their crates.
    pub fn counted_bases(&self) -> impl Iterator<Item = (&'i str, i8)> + '_ {
        self.counted
            .iter()
            .map_while(|c| c.map(|c| (c.name, c.exp)))
    }

    /// Without any exponent: a ratio.
    pub const fn is_one(&self) -> bool {
        self.same(Dim::ONE)
    }

    /// `self^times · o^sign`, `None` when an exponent leaves -8..=7 or more than [MAX_COUNTED] counted bases are left.
    /// A side without `root` is squared first when the other has it; the result is made canonical (halved, without
    /// `root`, when every exponent is even).
    const fn combine(self, o: Dim<'i>, sign: i16, times: i16) -> Option<Dim<'i>> {
        // with only one side a root, the other is squared so both describe the square of what they are
        let (fa, fb) = match (self.root, o.root) {
            (false, true) => (2, 1),
            (true, false) => (1, 2),
            _ => (1, 1),
        };
        let a = self.exps();
        let b = o.exps();
        let mut fixed = [0i16; FIXED];
        let mut i = 0;
        while i < FIXED {
            fixed[i] = a[i] as i16 * fa * times + sign * b[i] as i16 * fb;
            i += 1;
        }
        // both lists are sorted: merge them, adding the exponents of a base in both
        let mut merged: [Option<(&'i str, &'i str, i16)>; MERGED] = [None; MERGED];
        let (mut i, mut j, mut n) = (0, 0, 0);
        loop {
            let (c, e) = match (self.slot(i), o.slot(j)) {
                (None, None) => break,
                (Some((ca, ea)), None) => {
                    i += 1;
                    (ca, ea as i16 * fa * times)
                }
                (None, Some((cb, eb))) => {
                    j += 1;
                    (cb, sign * eb as i16 * fb)
                }
                (Some((ca, ea)), Some((cb, eb))) => {
                    match cmp_bases((ca.crate_name, ca.name), (cb.crate_name, cb.name)) {
                        core::cmp::Ordering::Less => {
                            i += 1;
                            (ca, ea as i16 * fa * times)
                        }
                        core::cmp::Ordering::Greater => {
                            j += 1;
                            (cb, sign * eb as i16 * fb)
                        }
                        core::cmp::Ordering::Equal => {
                            i += 1;
                            j += 1;
                            (ca, ea as i16 * fa * times + sign * eb as i16 * fb)
                        }
                    }
                }
            };
            if e != 0 {
                merged[n] = Some((c.crate_name, c.name, e));
                n += 1;
            }
        }
        let mut root = self.root || o.root;
        if root {
            // a square root of exponents that are all even is a plain dimension
            let mut all_even = true;
            let mut i = 0;
            while i < FIXED {
                all_even &= fixed[i] % 2 == 0;
                i += 1;
            }
            let mut i = 0;
            while i < n {
                if let Some((_, _, e)) = merged[i] {
                    all_even &= e % 2 == 0;
                }
                i += 1;
            }
            if all_even {
                root = false;
                let mut i = 0;
                while i < FIXED {
                    fixed[i] /= 2;
                    i += 1;
                }
                let mut i = 0;
                while i < n {
                    if let Some((c, name, e)) = merged[i] {
                        merged[i] = Some((c, name, e / 2));
                    }
                    i += 1;
                }
            }
        }
        if n > MAX_COUNTED {
            return None;
        }
        let mut exps = [0i8; FIXED];
        let mut i = 0;
        while i < FIXED {
            match checked_exp(fixed[i]) {
                Some(e) => exps[i] = e,
                None => return None,
            }
            i += 1;
        }
        let mut out = Dim::from_fixed(exps);
        out.root = root;
        let mut i = 0;
        while i < n {
            if let Some((crate_name, name, e)) = merged[i] {
                let Some(exp) = checked_exp(e) else {
                    return None;
                };
                out.counted[i] = Some(Counted {
                    crate_name,
                    name,
                    exp,
                });
            }
            i += 1;
        }
        Some(out)
    }

    const fn slot(&self, i: usize) -> Option<(Counted<'i>, i8)> {
        if i >= MAX_COUNTED {
            return None;
        }
        match self.counted[i] {
            Some(c) => Some((c, c.exp)),
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

    /// The dimension of a square root: halved exponents when they are all even, else the same exponents with
    /// `root`. `None` for the root of a root (a fourth root isn't a `Dim`).
    pub const fn checked_sqrt(self) -> Option<Dim<'i>> {
        if self.root {
            return None;
        }
        let mut squared_root = self;
        squared_root.root = true;
        // canonical form through combine: halves when every exponent is even
        squared_root.combine(Dim::new(0, 0, 0, 0, 0, 0, 0), 0, 1)
    }

    /// The dimension of a square root, for unit markers: V/√Hz is `Volt.div(Hertz.sqrt())`. Panics (a compile
    /// error in `const` code) where [checked_sqrt](Self::checked_sqrt) is `None`.
    pub const fn sqrt(self) -> Dim<'i> {
        match self.checked_sqrt() {
            Some(d) => d,
            None => panic!("square root of a dimension that is already a root"),
        }
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
        if self.root != o.root {
            return false;
        }
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
                    let same = matches!(
                        cmp_bases((x.crate_name, x.name), (y.crate_name, y.name)),
                        core::cmp::Ordering::Equal
                    );
                    if !same || x.exp != y.exp {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// Whether the dimension is as it must be on the wire: counted bases in used slots first, sorted by crate and
    /// name without duplicates, exponents not 0; `root` only with an odd exponent.
    pub const fn is_canonical(&self) -> bool {
        if self.root {
            let mut odd = false;
            let e = self.exps();
            let mut i = 0;
            while i < FIXED {
                odd |= e[i] % 2 != 0;
                i += 1;
            }
            let mut i = 0;
            while i < MAX_COUNTED {
                if let Some(c) = self.counted[i] {
                    odd |= c.exp % 2 != 0;
                }
                i += 1;
            }
            if !odd {
                return false;
            }
        }
        let mut i = 0;
        let mut ended = false;
        while i < MAX_COUNTED {
            match self.counted[i] {
                None => ended = true,
                Some(c) => {
                    if ended || c.exp == 0 {
                        return false;
                    }
                    if i > 0
                        && let Some(prev) = self.counted[i - 1]
                    {
                        let order = cmp_bases((prev.crate_name, prev.name), (c.crate_name, c.name));
                        if !matches!(order, core::cmp::Ordering::Less) {
                            return false;
                        }
                    }
                }
            }
            i += 1;
        }
        true
    }

    #[cfg(feature = "shrink_wrap")]
    fn validate(&self) -> Result<(), ShrinkWrapError> {
        if self.is_canonical() {
            Ok(())
        } else {
            Err(ShrinkWrapError::SubtypeOutOfRange)
        }
    }
}

/// Written the SI way, base units in the order kg, m, s, A, K, mol, cd, then rad, bit and the counted bases:
/// `kg·m²·s⁻³·A⁻¹`, `rad·s⁻¹`, `Sample·s⁻¹`, `1` when dimensionless; a root with halves: `kg·m²·s⁻⁵ᐟ²·A⁻¹`.
impl fmt::Display for Dim<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts = [
            ("kg", self.kg),
            ("m", self.m),
            ("s", self.s),
            ("A", self.a),
            ("K", self.k),
            ("mol", self.mol),
            ("cd", self.cd),
            ("rad", self.rad),
            ("bit", self.bit),
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
            if self.root && exp % 2 != 0 {
                write_superscript(f, exp)?;
                f.write_str("ᐟ²")?;
            } else {
                let exp = if self.root { exp / 2 } else { exp };
                if exp != 1 {
                    write_superscript(f, exp)?;
                }
            }
        }
        if first {
            f.write_str("1")?;
        }
        Ok(())
    }
}

#[cfg(all(feature = "std", feature = "shrink_wrap"))]
impl DimOwned {
    fn validate(&self) -> Result<(), ShrinkWrapError> {
        self.as_dim().validate()
    }

    /// The same dimension, borrowing the names of counted bases.
    pub fn as_dim(&self) -> Dim<'_> {
        let mut counted = [None; MAX_COUNTED];
        for (slot, c) in counted.iter_mut().zip(&self.counted) {
            *slot = c.as_ref().map(|c| Counted {
                crate_name: c.crate_name.as_str(),
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
            root: self.root,
            counted,
        }
    }
}

#[cfg(all(feature = "std", feature = "shrink_wrap"))]
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
            root: d.root,
            counted: d.counted.map(|c| {
                c.map(|c| CountedOwned {
                    crate_name: c.crate_name.into(),
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
#[cfg_attr(
    feature = "shrink_wrap",
    derive_shrink_wrap(
        borrowed,
        owned(feature = "std"),
        ww_repr = u4,
        // part of a unit's description: a reader can't convert with a scale it doesn't know
        no_future_version
    )
)]
#[derive(Copy, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Scale {
    /// The unit is coherent SI: V, A, N·m, rad.
    Coherent,
    /// A power of ten: g is 10⁻³ kg, L is 10⁻³ m³.
    Pow10(i8),
    /// A factor and an offset: °C is K − 273.15, `Affine { factor: 1.0, offset: 273.15 }`.
    Affine { factor: f64, offset: f64 },
    /// An exact factor, integers and a power of π: ° is π/180 rad, a turn 2π rad, rpm π/30 rad/s, eV
    /// 1.602176634·10⁻¹⁹ J. Every reader computes the same value.
    Exact(crate::Magnitude),
}

impl Scale {
    /// The factor, without the offset of an `Affine` scale.
    pub const fn factor(&self) -> f64 {
        match *self {
            Scale::Coherent => 1.0,
            Scale::Pow10(exp) => crate::num::pow10_f64(exp),
            Scale::Affine { factor, .. } => factor,
            Scale::Exact(scale) => scale.to_f64(),
        }
    }

    /// The factor exactly: `None` for an `Affine` scale.
    pub const fn magnitude(&self) -> Option<crate::Magnitude> {
        match *self {
            Scale::Coherent => Some(crate::Magnitude::ONE),
            Scale::Pow10(e) => crate::Magnitude::checked_pow10(e),
            Scale::Exact(m) => Some(m),
            Scale::Affine { .. } => None,
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

/// Where a named unit's scale comes from.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Provenance {
    /// The scale is the unit's definition: exact by law or by the SI (a foot is 0.3048 m).
    Defined,
    /// The unit is fixed by a physical property (mercury at 32 °F, a calorie at 15 °C): its scale is the value a
    /// source prints, exact in this crate but only by convention, and a newer measurement may differ.
    Conventional {
        /// The document the value is from: `"NIST SP 811"`.
        source: &'static str,
        /// Its edition.
        year: u16,
    },
}

/// A unit as a tree: named units, and products and quotients of them (`Per<Torque, AngularVelocity>` is
/// `Div(Torque, AngularVelocity)`). Every [Unit] has one in [Unit::EXPR]; [units::by_name](crate::units::by_name)
/// finds the named ones of this crate.
// Only ever in `static`s and consts, never moved around: boxing the named variant would buy nothing
#[allow(clippy::large_enum_variant)]
#[derive(Debug, PartialEq)]
pub enum UnitExpr {
    Named {
        /// The marker's name: `"Volt"`.
        name: &'static str,
        /// `"V"`, empty for [Ratio](crate::units::Ratio).
        symbol: &'static str,
        dim: Dim<'static>,
        scale: Scale,
        /// Whether the scale is the unit's definition or a measured value written down by convention.
        provenance: Provenance,
    },
    Mul(&'static UnitExpr, &'static UnitExpr),
    Div(&'static UnitExpr, &'static UnitExpr),
    /// A unit to an integer power: m³ as `Pow(Meter, 3)`. No marker of this crate is one, a user's marker can be.
    Pow(&'static UnitExpr, i8),
    /// The square root of a unit: √Hz in V/√Hz, [Sqrt](crate::units::Sqrt). Its dimension has the root flag
    /// ([Dim::sqrt]) unless every exponent is even.
    Sqrt(&'static UnitExpr),
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
            UnitExpr::Sqrt(a) => a.dim().sqrt(),
        }
    }

    /// The factor to coherent SI of the whole expression (offsets left out: a product with °C is not affine).
    pub const fn factor(&self) -> f64 {
        match self {
            UnitExpr::Named { scale, .. } => scale.factor(),
            UnitExpr::Mul(a, b) => a.factor() * b.factor(),
            UnitExpr::Div(a, b) => a.factor() / b.factor(),
            UnitExpr::Pow(a, n) => crate::num::powi_f64(a.factor(), *n),
            UnitExpr::Sqrt(a) => crate::num::sqrt_f64(a.factor()),
        }
    }

    /// The exact factor to coherent SI of the whole expression: `None` when a named unit in it has an
    /// `Affine` scale, or a product leaves what a [Magnitude](crate::Magnitude) holds. A pound-force second is
    /// 0.45359237 · 9.80665 N·s.
    pub const fn magnitude(&self) -> Option<crate::Magnitude> {
        use crate::Magnitude;
        match self {
            UnitExpr::Named { scale, .. } => scale.magnitude(),
            UnitExpr::Mul(a, b) => match (a.magnitude(), b.magnitude()) {
                (Some(a), Some(b)) => ok(a.checked_mul(b)),
                _ => None,
            },
            UnitExpr::Div(a, b) => match (a.magnitude(), b.magnitude()) {
                (Some(a), Some(b)) => ok(a.checked_div(b)),
                _ => None,
            },
            UnitExpr::Pow(a, n) => match a.magnitude() {
                Some(a) => {
                    // powi panics out of range: go through checked products
                    let mut r = Magnitude::ONE;
                    let mut i = 0;
                    while i < n.unsigned_abs() {
                        r = match r.checked_mul(a) {
                            Ok(r) => r,
                            Err(_) => return None,
                        };
                        i += 1;
                    }
                    if *n < 0 {
                        ok(Magnitude::ONE.checked_div(r))
                    } else {
                        Some(r)
                    }
                }
                None => None,
            },
            UnitExpr::Sqrt(a) => match a.magnitude() {
                Some(a) => ok(a.checked_sqrt()),
                None => None,
            },
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
            UnitExpr::Pow(a, _) | UnitExpr::Sqrt(a) => a.has_offset(),
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

/// `Result::ok` in `const` code.
const fn ok(r: Result<crate::Magnitude, crate::MagnitudeError>) -> Option<crate::Magnitude> {
    match r {
        Ok(m) => Some(m),
        Err(_) => None,
    }
}

/// The symbol: `V`, `N·m/(rad/s)`, `(rad/s)·(rad/s)`.
impl fmt::Display for UnitExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&tree::symbol(&self), f)
    }
}

/// A unit marker: a zero-size type that is never sent, only named in types (`Fixed<units::Volt, Milli, u16>`). The markers of
/// this crate are in [units](crate::units); an API crate declares its own the same way, with [unit!](crate::unit):
///
/// ```
/// use exact_units::{Fixed, Unit, prefix::One};
///
/// exact_units::unit! {
///     /// Revolutions per minute: 2π rad / 60 s, exactly.
///     pub struct Rpm = "rpm", [rad 1, s -1], Exact(1 / 30, pi 1);
/// }
///
/// let speed = Fixed::<Rpm, One, u8>::new(60);
/// assert_eq!(format!("{speed}"), "60 rpm");
/// assert!((<Rpm as Unit>::EXPR.factor() * 60.0 - core::f64::consts::TAU).abs() < 1e-12); // rad/s
/// ```
///
/// A hand-written impl compiles too, but WireWeaver refuses an API that uses such a marker: its walker reads a unit's
/// definition from the `unit!` line, so that clients know it (INT-13).
pub trait Unit: Copy + Default + 'static {
    /// The dimension: SI base-unit exponents, angle, information and counted bases.
    const DIM: Dim<'static>;
    /// Name, symbol and scale, or the tree of a compound unit.
    const EXPR: &'static UnitExpr;
    /// The same unit as it goes on the wire in an `ww_si::AnyQuantity`. Made from [EXPR](Self::EXPR)
    /// for a named unit; a compound marker of its own (not `Per` / `Times`) writes its tree here too. With the
    /// `shrink_wrap` feature.
    #[cfg(feature = "shrink_wrap")]
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

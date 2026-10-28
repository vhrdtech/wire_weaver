//! [Magnitude]: one canonical exact factor for every scale, a product of prime powers and powers of π, ln 2 and
//! ln 10, so every reader computes the same value from it and two magnitudes compare equal however they were spelled
//! (`Milli`, `Pow10<-3>`, `Rational<1, 1000>`).

#[cfg(feature = "shrink_wrap")]
use shrink_wrap::prelude::*;

/// How many primes other than 2 and 5 a [Magnitude] holds. Every imperial and US customary unit takes at most 7 of
/// them (the slug: 3, 7, 11, 97, 127, 6073, 28019), any product or quotient of two of them at most 10.
pub const MAX_FACTORS: usize = 12;

/// An exact positive factor: `10^a · 2^b · π^c · ln2^d · ln10^e · ∏ pᵢ^kᵢ` over primes `pᵢ` other than 2 and 5, every
/// exponent a whole or half number (`√2`, `10^(3/2)` for √(kHz)). `10⁻⁶` (µ), `2¹⁰` (Ki), `1/65535` (a 16-bit duty,
/// `3⁻¹ · 5⁻¹ · 17⁻¹ · 257⁻¹`), `π/180` (°), the pound's `0.45359237` (`7 · 11 · 97 · 6073 · 10⁻⁸`), `ln10/20` (a
/// decibel in neper). Never an `f64`, so every client reproduces a value bit-exactly, and a scale fixed in a type
/// ([Fixed](crate::Fixed)'s `P`) converts integers without a rounding step.
///
/// Closed under multiplication and division: a product only fails when it needs more than [MAX_FACTORS] primes or
/// an exponent leaves ±63½, and then at compile time in a `const` (a panic at run time; the `checked_` forms say
/// `None`), never by rounding.
///
/// Always canonical, so equal magnitudes are equal field by field: 2 and 5 only in `pow10` / `pow2` (`5 = 10 · 2⁻¹`),
/// the other primes sorted with exponents that aren't 0, and `rest` / `irrational` left out when they are 1. Plain
/// `const` data without lifetimes, so a `&'static Magnitude` can be held at run time; [Fixed](crate::Fixed) uses it
/// only at compile time.
///
/// On the wire every exponent is twice the exponent as an `i8`, the primes as [UNib32]: `pow10` alone is 9 bits
/// (Milli `fa 00`), the rest behind an option bit, its irrational powers behind one more, an empty prime slot one bit.
#[cfg_attr(
    feature = "shrink_wrap",
    derive_shrink_wrap(borrowed, owned(feature = "std"), final_structure, validate = Self::validate)
)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Magnitude {
    /// Twice the exponent of 10.
    pow10: i8,
    /// Everything but the power of 10, `None` when it is 1.
    rest: Option<MagnitudeRest>,
}

/// The part of a [Magnitude] after its power of 10: rarer, so it costs one bit on the wire when it is 1.
#[cfg_attr(
    feature = "shrink_wrap",
    derive_shrink_wrap(borrowed, owned(feature = "std"), final_structure)
)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct MagnitudeRest {
    /// Twice the exponent of 2.
    pow2: i8,
    /// π, ln 2 and ln 10, `None` when all their exponents are 0.
    irrational: Option<Irrational>,
    /// Primes other than 2 and 5, ascending, used slots first.
    factors: [Option<PrimePower>; MAX_FACTORS],
}

/// The powers of π, ln 2 and ln 10 in a [Magnitude], each twice the exponent: `π/180` has `pi = 2`, a decibel in
/// neper (`ln10/20`) `ln10 = 2`. Treated as independent symbols, like the primes.
#[cfg_attr(
    feature = "shrink_wrap",
    derive_shrink_wrap(borrowed, owned(feature = "std"), final_structure)
)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Irrational {
    pub pi: i8,
    pub ln2: i8,
    pub ln10: i8,
}

/// `prime^(exp/2)` in a [Magnitude]: a prime other than 2 and 5, and twice its exponent (never 0).
#[cfg_attr(
    feature = "shrink_wrap",
    derive_shrink_wrap(borrowed, owned(feature = "std"), final_structure)
)]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
// 6 bytes, so an `Option` of it is 8: half the size of a `Magnitude`. Packed, so `defmt::Format` is written out below
// (a derive would take references to its fields).
#[repr(C, packed(2))]
pub struct PrimePower {
    #[cfg_attr(feature = "shrink_wrap", wire(UNib32))]
    pub prime: u32,
    pub exp: i8,
}

#[cfg(feature = "defmt")]
impl defmt::Format for PrimePower {
    fn format(&self, f: defmt::Formatter) {
        let (prime, exp) = (self.prime, self.exp);
        defmt::write!(f, "PrimePower {{ prime: {}, exp: {} }}", prime, exp)
    }
}

/// Why a [Magnitude] can't be made.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum MagnitudeError {
    /// A factor of 0: a magnitude is positive.
    Zero,
    /// A prime factor above `u32::MAX` (only from a `u64` written by hand; no unit definition has one).
    PrimeTooLarge,
    /// More than [MAX_FACTORS] primes other than 2 and 5.
    TooManyFactors,
    /// An exponent outside ±63½.
    ExponentRange,
    /// The square root of an exponent that is already a half (a quarter power).
    NotSquare,
    /// A prime factor given by its parts that isn't a prime other than 2 and 5 ([Magnitude::from_parts_x2]).
    NotPrime,
}

impl MagnitudeError {
    const fn panic(self) -> ! {
        match self {
            MagnitudeError::Zero => panic!("a magnitude is not zero"),
            MagnitudeError::PrimeTooLarge => {
                panic!("a prime factor of a magnitude is above u32::MAX")
            }
            MagnitudeError::TooManyFactors => {
                panic!("a magnitude has more than MAX_FACTORS primes other than 2 and 5")
            }
            MagnitudeError::ExponentRange => panic!("an exponent of a magnitude is out of ±63½"),
            MagnitudeError::NotSquare => {
                panic!("the square root of a magnitude with a half exponent")
            }
            MagnitudeError::NotPrime => panic!("a prime factor of a magnitude is not a prime"),
        }
    }
}

/// A magnitude while it is computed: exponents doubled in `i32`, room for more primes than a [Magnitude] holds.
#[derive(Copy, Clone)]
struct Work {
    pow10: i32,
    pow2: i32,
    irr: [i32; 3],
    primes: [u32; WORK],
    exps: [i32; WORK],
    len: usize,
}

/// Two full magnitudes, or two `u64` of up to 15 distinct primes each.
const WORK: usize = 32;

impl Work {
    const ONE: Work = Work {
        pow10: 0,
        pow2: 0,
        irr: [0; 3],
        primes: [0; WORK],
        exps: [0; WORK],
        len: 0,
    };

    const fn of(m: &Magnitude) -> Work {
        let mut w = Work::ONE;
        w.pow10 = m.pow10 as i32;
        if let Some(rest) = &m.rest {
            w.pow2 = rest.pow2 as i32;
            if let Some(irr) = &rest.irrational {
                w.irr = [irr.pi as i32, irr.ln2 as i32, irr.ln10 as i32];
            }
            let mut i = 0;
            while i < MAX_FACTORS {
                if let Some(f) = rest.factors[i] {
                    w.primes[w.len] = f.prime;
                    w.exps[w.len] = f.exp as i32;
                    w.len += 1;
                }
                i += 1;
            }
        }
        w
    }

    /// `prime^(exp2 / 2)` multiplied in, `prime` a prime.
    const fn add_prime(&mut self, prime: u64, exp2: i32) -> Result<(), MagnitudeError> {
        if prime == 2 {
            self.pow2 += exp2;
            return Ok(());
        }
        if prime == 5 {
            // 5 = 10 · 2⁻¹
            self.pow10 += exp2;
            self.pow2 -= exp2;
            return Ok(());
        }
        if prime > u32::MAX as u64 {
            return Err(MagnitudeError::PrimeTooLarge);
        }
        let prime = prime as u32;
        let mut i = 0;
        while i < self.len && self.primes[i] < prime {
            i += 1;
        }
        if i < self.len && self.primes[i] == prime {
            self.exps[i] += exp2;
            if self.exps[i] == 0 {
                // drop the slot
                while i + 1 < self.len {
                    self.primes[i] = self.primes[i + 1];
                    self.exps[i] = self.exps[i + 1];
                    i += 1;
                }
                self.len -= 1;
            }
            return Ok(());
        }
        if self.len == WORK {
            return Err(MagnitudeError::TooManyFactors);
        }
        let mut j = self.len;
        while j > i {
            self.primes[j] = self.primes[j - 1];
            self.exps[j] = self.exps[j - 1];
            j -= 1;
        }
        self.primes[i] = prime;
        self.exps[i] = exp2;
        self.len += 1;
        Ok(())
    }

    /// `n^(exp2 / 2)` multiplied in, `n` factored into primes.
    const fn add_int(&mut self, n: u64, exp2: i32) -> Result<(), MagnitudeError> {
        if n == 0 {
            return Err(MagnitudeError::Zero);
        }
        let mut n = n;
        let mut d = 2u64;
        // trial division up to 2¹⁶: 2, 3, then 6k ± 1
        while d <= 0xffff && d * d <= n {
            while n.is_multiple_of(d) {
                n /= d;
                if let Err(e) = self.add_prime(d, exp2) {
                    return Err(e);
                }
            }
            d = match d {
                2 => 3,
                3 => 5,
                _ if d % 6 == 5 => d + 2,
                _ => d + 4,
            };
        }
        if n == 1 {
            return Ok(());
        }
        // no factor up to min(√n, 2¹⁶) is left: below 2³², n is a prime
        self.add_large(n, exp2)
    }

    /// `n` with no prime factor below 2¹⁶: a prime, or a product of up to three larger ones.
    const fn add_large(&mut self, n: u64, exp2: i32) -> Result<(), MagnitudeError> {
        if n <= u32::MAX as u64 || is_prime(n) {
            return self.add_prime(n, exp2);
        }
        let f = rho(n);
        if let Err(e) = self.add_large(f, exp2) {
            return Err(e);
        }
        self.add_large(n / f, exp2)
    }

    const fn mul(mut self, rhs: &Work, sign: i32) -> Result<Work, MagnitudeError> {
        self.pow10 += sign * rhs.pow10;
        self.pow2 += sign * rhs.pow2;
        let mut i = 0;
        while i < 3 {
            self.irr[i] += sign * rhs.irr[i];
            i += 1;
        }
        let mut i = 0;
        while i < rhs.len {
            if let Err(e) = self.add_prime(rhs.primes[i] as u64, sign * rhs.exps[i]) {
                return Err(e);
            }
            i += 1;
        }
        Ok(self)
    }

    /// Every exponent times `n` (`n` 1 for a square root, after halving).
    const fn scale(mut self, n: i32) -> Work {
        self.pow10 *= n;
        self.pow2 *= n;
        let mut i = 0;
        while i < 3 {
            self.irr[i] *= n;
            i += 1;
        }
        let mut i = 0;
        while i < self.len {
            self.exps[i] *= n;
            i += 1;
        }
        self
    }

    const fn pack(&self) -> Result<Magnitude, MagnitudeError> {
        if self.len > MAX_FACTORS {
            return Err(MagnitudeError::TooManyFactors);
        }
        let pow10 = match exp(self.pow10) {
            Ok(e) => e,
            Err(e) => return Err(e),
        };
        let pow2 = match exp(self.pow2) {
            Ok(e) => e,
            Err(e) => return Err(e),
        };
        let mut irr = [0i8; 3];
        let mut i = 0;
        while i < 3 {
            irr[i] = match exp(self.irr[i]) {
                Ok(e) => e,
                Err(e) => return Err(e),
            };
            i += 1;
        }
        let mut factors = [None; MAX_FACTORS];
        let mut i = 0;
        while i < self.len {
            factors[i] = Some(PrimePower {
                prime: self.primes[i],
                exp: match exp(self.exps[i]) {
                    Ok(e) => e,
                    Err(e) => return Err(e),
                },
            });
            i += 1;
        }
        let irrational = if irr[0] == 0 && irr[1] == 0 && irr[2] == 0 {
            None
        } else {
            Some(Irrational {
                pi: irr[0],
                ln2: irr[1],
                ln10: irr[2],
            })
        };
        let rest = if pow2 == 0 && irrational.is_none() && self.len == 0 {
            None
        } else {
            Some(MagnitudeRest {
                pow2,
                irrational,
                factors,
            })
        };
        Ok(Magnitude { pow10, rest })
    }
}

/// A doubled exponent as it is stored.
const fn exp(e: i32) -> Result<i8, MagnitudeError> {
    if e < i8::MIN as i32 || e > i8::MAX as i32 {
        Err(MagnitudeError::ExponentRange)
    } else {
        Ok(e as i8)
    }
}

/// `a · b mod m`.
const fn mul_mod(a: u64, b: u64, m: u64) -> u64 {
    ((a as u128 * b as u128) % m as u128) as u64
}

const fn pow_mod(mut b: u64, mut e: u64, m: u64) -> u64 {
    let mut r = 1 % m;
    b %= m;
    while e > 0 {
        if e & 1 == 1 {
            r = mul_mod(r, b, m);
        }
        b = mul_mod(b, b, m);
        e >>= 1;
    }
    r
}

/// Miller-Rabin with the bases that decide every `u64`.
pub(crate) const fn is_prime(n: u64) -> bool {
    if n < 2 {
        return false;
    }
    const BASES: [u64; 12] = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37];
    let mut i = 0;
    while i < BASES.len() {
        if n == BASES[i] {
            return true;
        }
        if n.is_multiple_of(BASES[i]) {
            return false;
        }
        i += 1;
    }
    let mut d = n - 1;
    let mut s = 0;
    while d.is_multiple_of(2) {
        d /= 2;
        s += 1;
    }
    let mut i = 0;
    'bases: while i < BASES.len() {
        let mut x = pow_mod(BASES[i], d, n);
        i += 1;
        if x == 1 || x == n - 1 {
            continue;
        }
        let mut r = 1;
        while r < s {
            x = mul_mod(x, x, n);
            if x == n - 1 {
                continue 'bases;
            }
            r += 1;
        }
        return false;
    }
    true
}

const fn gcd64(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

/// A factor of the odd composite `n` other than 1 and `n` (Pollard's rho with Brent's cycle finding).
const fn rho(n: u64) -> u64 {
    let mut c = 1;
    loop {
        let (mut x, mut y, mut g) = (2u64, 2u64, 1u64);
        while g == 1 {
            x = (mul_mod(x, x, n) + c) % n;
            y = (mul_mod(y, y, n) + c) % n;
            y = (mul_mod(y, y, n) + c) % n;
            g = gcd64(x.abs_diff(y), n);
        }
        if g != n {
            return g;
        }
        c += 1;
    }
}

impl Magnitude {
    /// 1.
    pub const ONE: Magnitude = Magnitude {
        pow10: 0,
        rest: None,
    };

    /// π.
    pub const PI: Magnitude = Self::irrational(2, 0, 0);

    /// ln 2: a shannon (bit) in nat is ln 2, an octave in neper-like units too.
    pub const LN2: Magnitude = Self::irrational(0, 2, 0);

    /// ln 10: a decibel is ln10/20 Np, a hartley ln 10 nat.
    pub const LN10: Magnitude = Self::irrational(0, 0, 2);

    const fn irrational(pi: i8, ln2: i8, ln10: i8) -> Magnitude {
        Magnitude {
            pow10: 0,
            rest: Some(MagnitudeRest {
                pow2: 0,
                irrational: Some(Irrational { pi, ln2, ln10 }),
                factors: [None; MAX_FACTORS],
            }),
        }
    }

    /// `10^e`: a decimal prefix. Panics (at compile time in a `const`) for `|e| > 63`.
    pub const fn pow10(e: i8) -> Self {
        match Self::checked_pow10(e) {
            Some(m) => m,
            None => MagnitudeError::ExponentRange.panic(),
        }
    }

    /// `10^e`, `None` for `|e| > 63`.
    pub const fn checked_pow10(e: i8) -> Option<Self> {
        match exp(2 * e as i32) {
            Ok(pow10) => Some(Magnitude { pow10, rest: None }),
            Err(_) => None,
        }
    }

    /// `2^e`: a binary prefix, a Q format. Panics (at compile time in a `const`) for `|e| > 63`.
    pub const fn pow2(e: i8) -> Self {
        let mut w = Work::ONE;
        w.pow2 = 2 * e as i32;
        unwrap(w.pack())
    }

    /// `num / den`, factored into primes. Panics (at compile time in a `const`) where [checked_ratio](Self::checked_ratio)
    /// is an error.
    pub const fn ratio(num: u64, den: u64) -> Self {
        unwrap(Self::checked_ratio(num, den))
    }

    /// `num / den`, factored into primes; an error for 0, a prime factor above `u32::MAX`, too many primes or an
    /// exponent out of range.
    pub const fn checked_ratio(num: u64, den: u64) -> Result<Self, MagnitudeError> {
        let mut w = Work::ONE;
        if let Err(e) = w.add_int(num, 2) {
            return Err(e);
        }
        if let Err(e) = w.add_int(den, -2) {
            return Err(e);
        }
        w.pack()
    }

    /// `num / den · 10^pow10 · 2^pow2 · π^pi`: how the scales of unit definitions are written (`°` is
    /// `new(1, 180, 0, 0, 1)`). Panics (at compile time in a `const`) when that isn't a magnitude.
    pub const fn new(num: u64, den: u64, pow10: i8, pow2: i8, pi: i8) -> Self {
        Self::ratio(num, den)
            .mul(Self::pow10(pow10))
            .mul(Self::pow2(pow2))
            .mul(Self::PI.powi(pi))
    }

    /// `self · rhs`. Panics (at compile time in a `const`) where [checked_mul](Self::checked_mul) is `None`.
    pub const fn mul(self, rhs: Self) -> Self {
        unwrap(self.checked_mul(rhs))
    }

    /// `self · rhs`, `None` with more than [MAX_FACTORS] primes or an exponent out of range.
    pub const fn checked_mul(self, rhs: Self) -> Result<Self, MagnitudeError> {
        match Work::of(&self).mul(&Work::of(&rhs), 1) {
            Ok(w) => w.pack(),
            Err(e) => Err(e),
        }
    }

    /// `self / rhs`. Panics (at compile time in a `const`) where [checked_div](Self::checked_div) is `None`.
    pub const fn div(self, rhs: Self) -> Self {
        unwrap(self.checked_div(rhs))
    }

    /// `self / rhs`, an error with more than [MAX_FACTORS] primes or an exponent out of range.
    pub const fn checked_div(self, rhs: Self) -> Result<Self, MagnitudeError> {
        match Work::of(&self).mul(&Work::of(&rhs), -1) {
            Ok(w) => w.pack(),
            Err(e) => Err(e),
        }
    }

    /// `1 / self`.
    pub const fn recip(self) -> Self {
        Self::ONE.div(self)
    }

    /// `self^n`. Panics (at compile time in a `const`) when an exponent leaves ±63½.
    pub const fn powi(self, n: i8) -> Self {
        unwrap(Work::of(&self).scale(n as i32).pack())
    }

    /// `√self`: exact, the exponents halved (√2, `10^(3/2)`). Panics (at compile time in a `const`) where
    /// [checked_sqrt](Self::checked_sqrt) is an error.
    pub const fn sqrt(self) -> Self {
        unwrap(self.checked_sqrt())
    }

    /// `√self`, [MagnitudeError::NotSquare] when an exponent is already a half (`√√2` is not a magnitude).
    pub const fn checked_sqrt(self) -> Result<Self, MagnitudeError> {
        let mut w = Work::of(&self);
        if w.pow10 % 2 != 0 || w.pow2 % 2 != 0 {
            return Err(MagnitudeError::NotSquare);
        }
        w.pow10 /= 2;
        w.pow2 /= 2;
        let mut i = 0;
        while i < 3 {
            if w.irr[i] % 2 != 0 {
                return Err(MagnitudeError::NotSquare);
            }
            w.irr[i] /= 2;
            i += 1;
        }
        let mut i = 0;
        while i < w.len {
            if w.exps[i] % 2 != 0 {
                return Err(MagnitudeError::NotSquare);
            }
            w.exps[i] /= 2;
            i += 1;
        }
        w.pack()
    }

    /// Twice the exponent of `prime` (2 and 5 included: `Milli` is 2⁻³ · 5⁻³, so -6 for both). 0 for a number that
    /// isn't a factor.
    pub const fn exp_x2(&self, prime: u32) -> i16 {
        let w = Work::of(self);
        match prime {
            2 => (w.pow10 + w.pow2) as i16,
            5 => w.pow10 as i16,
            _ => {
                let mut i = 0;
                while i < w.len {
                    if w.primes[i] == prime {
                        return w.exps[i] as i16;
                    }
                    i += 1;
                }
                0
            }
        }
    }

    /// Twice the exponents of π, ln 2 and ln 10.
    pub const fn irrational_x2(&self) -> Irrational {
        match &self.rest {
            Some(MagnitudeRest {
                irrational: Some(irr),
                ..
            }) => *irr,
            _ => Irrational {
                pi: 0,
                ln2: 0,
                ln10: 0,
            },
        }
    }

    /// Twice the exponent of 10 and of 2 as stored (`10^a · 2^b`, the 5s are in `a`).
    pub const fn pow10_pow2_x2(&self) -> (i8, i8) {
        match &self.rest {
            Some(rest) => (self.pow10, rest.pow2),
            None => (self.pow10, 0),
        }
    }

    /// The primes other than 2 and 5 with twice their exponents, ascending.
    pub fn primes(&self) -> impl Iterator<Item = PrimePower> + '_ {
        self.rest
            .iter()
            .flat_map(|r| r.factors.iter().flatten().copied())
    }

    /// The power of ten when the magnitude is one alone (`Some(-6)` for µ), `None` otherwise.
    pub const fn as_pow10(&self) -> Option<i8> {
        if self.rest.is_none() && self.pow10 % 2 == 0 {
            Some(self.pow10 / 2)
        } else {
            None
        }
    }

    /// Whether the magnitude is a fraction of integers: no power of π, ln 2 or ln 10, no half exponent.
    pub const fn is_rational(&self) -> bool {
        self.to_ratio().is_some()
    }

    /// `(num, den)` without a common factor, if the magnitude is a fraction of integers that both fit a `u128`.
    pub const fn to_ratio(&self) -> Option<(u128, u128)> {
        let w = Work::of(self);
        if w.irr[0] != 0 || w.irr[1] != 0 || w.irr[2] != 0 {
            return None;
        }
        let (mut num, mut den) = (1u128, 1u128);
        // 10^a · 2^b = 5^a · 2^(a + b)
        let mut bases = [0u32; WORK + 2];
        let mut exps = [0i32; WORK + 2];
        bases[0] = 2;
        exps[0] = w.pow10 + w.pow2;
        bases[1] = 5;
        exps[1] = w.pow10;
        let mut i = 0;
        while i < w.len {
            bases[i + 2] = w.primes[i];
            exps[i + 2] = w.exps[i];
            i += 1;
        }
        let mut i = 0;
        while i < w.len + 2 {
            let e = exps[i];
            if e % 2 != 0 {
                return None;
            }
            let p = match (bases[i] as u128).checked_pow(e.unsigned_abs() / 2) {
                Some(p) => p,
                None => return None,
            };
            if e > 0 {
                num = match num.checked_mul(p) {
                    Some(n) => n,
                    None => return None,
                };
            } else {
                den = match den.checked_mul(p) {
                    Some(d) => d,
                    None => return None,
                };
            }
            i += 1;
        }
        Some((num, den))
    }

    /// The factor as an `f64` (rounded, unlike the magnitude itself).
    pub const fn to_f64(&self) -> f64 {
        let w = Work::of(self);
        let mut x = pow_half(10.0, w.pow10) * pow_half(2.0, w.pow2);
        x *= pow_half(core::f64::consts::PI, w.irr[0]);
        x *= pow_half(core::f64::consts::LN_2, w.irr[1]);
        x *= pow_half(core::f64::consts::LN_10, w.irr[2]);
        let mut i = 0;
        while i < w.len {
            x *= pow_half(w.primes[i] as f64, w.exps[i]);
            i += 1;
        }
        x
    }

    /// `x · self` in integers, exactly: `None` when the result is not whole, doesn't fit `i128`, or the magnitude
    /// isn't a fraction of integers.
    pub fn apply_int(&self, x: i128) -> Option<i128> {
        let (num, den) = self.to_ratio()?;
        let (num, den) = (i128::try_from(num).ok()?, i128::try_from(den).ok()?);
        // num and den have no common factor: x · num / den is whole exactly when den divides x
        if x % den != 0 {
            return None;
        }
        (x / den).checked_mul(num)
    }

    /// `x · self` in floats.
    pub fn apply_f64(&self, x: f64) -> f64 {
        match self.to_ratio() {
            // exact factors through the integers, so 0.3 · 10 is 3.0 and not 3.0000000000000004
            Some((num, den)) if num <= 1 << 53 && den <= 1 << 53 => x * num as f64 / den as f64,
            _ => x * self.to_f64(),
        }
    }

    /// The magnitude of the parts [pow10_pow2_x2](Self::pow10_pow2_x2), [irrational_x2](Self::irrational_x2) and
    /// [primes](Self::primes) give: twice the exponents of 10 and 2, of π, ln 2 and ln 10, and primes other than 2
    /// and 5 with twice their exponents. In any order and with the same prime more than once; an error for a
    /// number in `primes` that isn't a prime, a result out of range, or a quarter power.
    pub fn from_parts_x2(
        pow10_x2: i8,
        pow2_x2: i8,
        irrational_x2: Irrational,
        primes: impl IntoIterator<Item = PrimePower>,
    ) -> Result<Self, MagnitudeError> {
        let mut w = Work::ONE;
        w.pow10 = pow10_x2 as i32;
        w.pow2 = pow2_x2 as i32;
        w.irr = [
            irrational_x2.pi as i32,
            irrational_x2.ln2 as i32,
            irrational_x2.ln10 as i32,
        ];
        for f in primes {
            if f.prime == 2 || f.prime == 5 || !is_prime(f.prime as u64) {
                return Err(MagnitudeError::NotPrime);
            }
            w.add_prime(f.prime as u64, f.exp as i32)?;
        }
        w.pack()
    }

    #[cfg(feature = "shrink_wrap")]
    fn validate(&self) -> Result<(), shrink_wrap::Error> {
        // canonical: rebuilt from its own parts it is the same
        let (pow10, pow2) = self.pow10_pow2_x2();
        if self.primes().any(|f| f.exp == 0) {
            return Err(shrink_wrap::Error::SubtypeOutOfRange);
        }
        match Self::from_parts_x2(pow10, pow2, self.irrational_x2(), self.primes()) {
            Ok(m) if m == *self => Ok(()),
            _ => Err(shrink_wrap::Error::SubtypeOutOfRange),
        }
    }
}

/// A [Magnitude] in serde (RON snapshots of APIs): its parts, each exponent doubled as on the wire, `primes` as
/// `(prime, exp_x2)`; rebuilt with [Magnitude::from_parts_x2], so a hand-edited file can't hold a non-canonical one.
#[cfg(feature = "serde")]
#[derive(serde::Serialize, serde::Deserialize)]
struct MagnitudeParts {
    pow10: i8,
    #[serde(default, skip_serializing_if = "is_zero")]
    pow2: i8,
    #[serde(default, skip_serializing_if = "is_zero")]
    pi: i8,
    #[serde(default, skip_serializing_if = "is_zero")]
    ln2: i8,
    #[serde(default, skip_serializing_if = "is_zero")]
    ln10: i8,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    primes: Vec<(u32, i8)>,
}

#[cfg(feature = "serde")]
fn is_zero(x: &i8) -> bool {
    *x == 0
}

#[cfg(feature = "serde")]
impl serde::Serialize for Magnitude {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (pow10, pow2) = self.pow10_pow2_x2();
        let irr = self.irrational_x2();
        MagnitudeParts {
            pow10,
            pow2,
            pi: irr.pi,
            ln2: irr.ln2,
            ln10: irr.ln10,
            primes: self.primes().map(|f| (f.prime, f.exp)).collect(),
        }
        .serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Magnitude {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let p = MagnitudeParts::deserialize(deserializer)?;
        Magnitude::from_parts_x2(
            p.pow10,
            p.pow2,
            Irrational {
                pi: p.pi,
                ln2: p.ln2,
                ln10: p.ln10,
            },
            p.primes
                .into_iter()
                .map(|(prime, exp)| PrimePower { prime, exp }),
        )
        .map_err(|e| serde::de::Error::custom(format_args!("{e:?}")))
    }
}

const fn unwrap(m: Result<Magnitude, MagnitudeError>) -> Magnitude {
    match m {
        Ok(m) => m,
        Err(e) => e.panic(),
    }
}

/// `x^(e2 / 2)` in `const` code.
const fn pow_half(x: f64, e2: i32) -> f64 {
    let whole = crate::num::powi_f64(x, (e2 / 2) as i8);
    if e2 % 2 == 0 {
        whole
    } else if e2 > 0 {
        whole * crate::num::sqrt_f64(x)
    } else {
        whole / crate::num::sqrt_f64(x)
    }
}

impl Default for Magnitude {
    fn default() -> Self {
        Self::ONE
    }
}

/// `·10⁻⁴`, `·2¹⁰`, `·1/65535`, `·π/180`, `·2π`, `·√2`, `·ln10/20`: the factor as written after a number. Empty for 1.
impl core::fmt::Display for Magnitude {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let (p10, p2) = self.pow10_pow2_x2();
        if *self == Self::ONE {
            return Ok(());
        }
        // a power alone: 10⁻⁴, 2¹⁰
        if let Some(e) = self.as_pow10() {
            f.write_str("·10")?;
            return write_superscript(f, e as i32);
        }
        if self
            .rest
            .is_some_and(|r| r.irrational.is_none() && r.factors[0].is_none())
            && p10 == 0
            && p2 % 2 == 0
        {
            f.write_str("·2")?;
            return write_superscript(f, (p2 / 2) as i32);
        }
        f.write_str("·")?;
        let w = Work::of(self);
        // whole and half parts of every prime power, numerator and denominator apart
        let (mut num, mut den, mut root_num, mut root_den) = (1u128, 1u128, 1u128, 1u128);
        let mut fits = true;
        let mut bases = [0u32; WORK + 2];
        let mut exps = [0i32; WORK + 2];
        (bases[0], exps[0], bases[1], exps[1]) = (2, w.pow10 + w.pow2, 5, w.pow10);
        for i in 0..w.len {
            (bases[i + 2], exps[i + 2]) = (w.primes[i], w.exps[i]);
        }
        for i in 0..w.len + 2 {
            let (p, e) = (bases[i] as u128, exps[i]);
            let (whole, half) = (e.unsigned_abs() / 2, e % 2 != 0);
            let (n, r) = if e > 0 {
                (&mut num, &mut root_num)
            } else {
                (&mut den, &mut root_den)
            };
            match p.checked_pow(whole).and_then(|x| n.checked_mul(x)) {
                Some(x) => *n = x,
                None => fits = false,
            }
            if half {
                *r *= p;
            }
        }
        if !fits || num > u64::MAX as u128 || den > u64::MAX as u128 {
            // too long as a fraction: the prime powers
            return write_powers(&bases[..w.len + 2], &exps[..w.len + 2], &w.irr, f);
        }
        let irr = [("π", w.irr[0]), ("ln2", w.irr[1]), ("ln10", w.irr[2])];
        let has_num_symbol = root_num != 1 || irr.iter().any(|(_, e)| *e > 0);
        if num != 1 || !has_num_symbol {
            write!(f, "{num}")?;
        }
        if root_num != 1 {
            write!(f, "√{root_num}")?;
        }
        write_symbols(&irr, 1, f)?;
        let has_den_symbol = root_den != 1 || irr.iter().any(|(_, e)| *e < 0);
        if den != 1 || has_den_symbol {
            f.write_str("/")?;
            if den != 1 || !has_den_symbol {
                write!(f, "{den}")?;
            }
            if root_den != 1 {
                write!(f, "√{root_den}")?;
            }
            write_symbols(&irr, -1, f)?;
        }
        Ok(())
    }
}

/// π, ln2, ln10 with the sign `sign` of their exponents: `π`, `π²`, `√π`, `π³ᐟ²`.
fn write_symbols(
    irr: &[(&str, i32); 3],
    sign: i32,
    f: &mut core::fmt::Formatter<'_>,
) -> core::fmt::Result {
    for (s, e) in irr {
        let e = e * sign;
        if e <= 0 {
            continue;
        }
        match e {
            1 => write!(f, "√{s}")?,
            2 => f.write_str(s)?,
            _ => {
                f.write_str(s)?;
                write_exp_x2(f, e)?;
            }
        }
    }
    Ok(())
}

/// Every power on its own: `2⁻¹⁶·3·7²·π`.
fn write_powers(
    bases: &[u32],
    exps: &[i32],
    irr: &[i32; 3],
    f: &mut core::fmt::Formatter<'_>,
) -> core::fmt::Result {
    let mut first = true;
    let names = ["π", "ln2", "ln10"];
    let all = bases
        .iter()
        .zip(exps)
        .map(|(b, e)| (Some(*b), "", *e))
        .chain(names.iter().zip(irr).map(|(n, e)| (None, *n, *e)));
    for (base, name, e) in all {
        if e == 0 {
            continue;
        }
        if !first {
            f.write_str("·")?;
        }
        first = false;
        match base {
            Some(b) => write!(f, "{b}")?,
            None => f.write_str(name)?,
        }
        if e != 2 {
            write_exp_x2(f, e)?;
        }
    }
    Ok(())
}

/// The exponent `e2 / 2` as a superscript: `²`, `⁻¹`, `³ᐟ²`.
fn write_exp_x2(f: &mut core::fmt::Formatter<'_>, e2: i32) -> core::fmt::Result {
    if e2 % 2 == 0 {
        write_superscript(f, e2 / 2)
    } else {
        write_superscript(f, e2)?;
        f.write_str("ᐟ²")
    }
}

fn write_superscript(f: &mut core::fmt::Formatter<'_>, e: i32) -> core::fmt::Result {
    const DIGITS: [&str; 10] = ["⁰", "¹", "²", "³", "⁴", "⁵", "⁶", "⁷", "⁸", "⁹"];
    if e < 0 {
        f.write_str("⁻")?;
    }
    let mut digits = [0u8; 10];
    let (mut e, mut n) = (e.unsigned_abs(), 0);
    loop {
        digits[n] = (e % 10) as u8;
        n += 1;
        e /= 10;
        if e == 0 {
            break;
        }
    }
    for d in digits[..n].iter().rev() {
        f.write_str(DIGITS[*d as usize])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;

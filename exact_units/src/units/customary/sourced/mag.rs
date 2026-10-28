//! Exact magnitudes: `num / den · 10^pow10 · 2^pow2 · π^pi`, in the form si-in's exact scales used, so the catalogue
//! moves into exact_units (STD-61) without converting numbers.

/// An exact positive magnitude, `num / den · 10^pow10 · 2^pow2 · π^pi`.
///
/// Canonical: `num` and `den` are coprime and have no factor 2 or 5 (each 5 taken out of `num` is `pow10 + 1, pow2 -
/// 1`), so equal magnitudes are equal structs however they were spelled. Every operation is a `const fn`: an overflow
/// of `num` or `den` panics, which in a `const` is a compile error, never a rounding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mag {
    pub num: u128,
    pub den: u128,
    pub pow10: i16,
    pub pow2: i16,
    pub pi: i8,
}

const fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

const fn checked(v: Option<u128>) -> u128 {
    match v {
        Some(v) => v,
        None => panic!("magnitude overflows u128"),
    }
}

impl Mag {
    pub const ONE: Mag = Mag::int(1);
    pub const PI: Mag = Mag {
        num: 1,
        den: 1,
        pow10: 0,
        pow2: 0,
        pi: 1,
    };

    /// `num / den · 10^pow10 · 2^pow2 · π^pi`, brought into canonical form.
    pub const fn new(mut num: u128, mut den: u128, mut pow10: i16, mut pow2: i16, pi: i8) -> Mag {
        assert!(num != 0 && den != 0, "a magnitude is positive");
        while num.is_multiple_of(2) {
            num /= 2;
            pow2 += 1;
        }
        while num.is_multiple_of(5) {
            num /= 5;
            pow10 += 1;
            pow2 -= 1;
        }
        while den.is_multiple_of(2) {
            den /= 2;
            pow2 -= 1;
        }
        while den.is_multiple_of(5) {
            den /= 5;
            pow10 -= 1;
            pow2 += 1;
        }
        let g = gcd(num, den);
        Mag {
            num: num / g,
            den: den / g,
            pow10,
            pow2,
            pi,
        }
    }

    pub const fn int(n: u128) -> Mag {
        Mag::new(n, 1, 0, 0, 0)
    }

    pub const fn ratio(num: u128, den: u128) -> Mag {
        Mag::new(num, den, 0, 0, 0)
    }

    /// A decimal as written in a source: `dec(45359237, -8)` is 0.45359237.
    pub const fn dec(digits: u128, exp10: i16) -> Mag {
        Mag::new(digits, 1, exp10, 0, 0)
    }

    pub const fn mul(self, o: Mag) -> Mag {
        // cross-reduce first, so a product that fits is never refused for an intermediate that doesn't
        let g1 = gcd(self.num, o.den);
        let g2 = gcd(o.num, self.den);
        let num = checked((self.num / g1).checked_mul(o.num / g2));
        let den = checked((self.den / g2).checked_mul(o.den / g1));
        Mag {
            num,
            den,
            pow10: self.pow10 + o.pow10,
            pow2: self.pow2 + o.pow2,
            pi: self.pi + o.pi,
        }
    }

    pub const fn recip(self) -> Mag {
        Mag {
            num: self.den,
            den: self.num,
            pow10: -self.pow10,
            pow2: -self.pow2,
            pi: -self.pi,
        }
    }

    pub const fn div(self, o: Mag) -> Mag {
        self.mul(o.recip())
    }

    pub const fn times(self, n: u128) -> Mag {
        self.mul(Mag::int(n))
    }

    pub const fn over(self, n: u128) -> Mag {
        self.div(Mag::int(n))
    }

    pub const fn powi(self, n: i32) -> Mag {
        let mut out = Mag::ONE;
        let base = if n < 0 { self.recip() } else { self };
        let mut i = 0;
        while i < n.unsigned_abs() {
            out = out.mul(base);
            i += 1;
        }
        out
    }

    /// Bits of `num` (2 and 5 already taken out).
    pub const fn num_bits(self) -> u32 {
        128 - self.num.leading_zeros()
    }

    /// Bits of `den` (2 and 5 already taken out).
    pub const fn den_bits(self) -> u32 {
        128 - self.den.leading_zeros()
    }
}

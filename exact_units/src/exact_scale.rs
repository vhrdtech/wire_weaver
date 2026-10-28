//! [ExactScale]: a one-byte handle for an exact scale that isn't a named prefix, so a [Prefix](crate::Prefix) stays
//! one byte in memory and carries the full [Magnitude] on the wire.

use crate::Magnitude;

/// How many handles there are: [COMPUTED] scales known to every build, the rest interned at run time on `std`.
pub const EXACT_SCALES: usize = 200;

/// Scales every build knows, computed from their index (no table of magnitudes in flash):
///
/// | index      | scale                                           |
/// |------------|-------------------------------------------------|
/// | `0..36`    | `10^e`, `e` in -30..=30, without the SI prefixes |
/// | `36..97`   | `2^e`, `e` in -32..=32, without 1, Ki, Mi, Gi     |
/// | `97..128`  | `1/(2^b - 1)`, `b` in 2..=32: full scales of `u8`, `i16`, ... (`1/65535`) |
/// | `128..162` | `π · 2^e`, `e` in -32..=1: BAM (`2π/2^n`), π, 2π |
pub const COMPUTED: usize = 162;

/// The most bytes a [COMPUTED] scale's [Magnitude] takes on the wire.
const COMPUTED_MAX_BYTES: usize = 20;

const POW10_START: u8 = 0;
const POW2_START: u8 = 36;
const FULL_START: u8 = 97;
const PI_START: u8 = 128;

/// An exact scale that isn't one of [Prefix](crate::Prefix)'s named ones: `1/65535` (a 16-bit duty), `10^-4`, `π/2^15`
/// (BAM), or on `std` any other [Magnitude] met at run time.
///
/// One byte in Rust, so `Prefix` and every quantity with it keep their size; on the wire the full magnitude, so the
/// bytes mean the same to every reader. [COMPUTED] scales are known to every build without a table in flash; on
/// `std` up to [EXACT_SCALES] − [COMPUTED] more are interned the first time they are met (deduplicated, kept for
/// the life of the process); past that, and without `std` for any scale outside the computed ones, there is no
/// handle ([ExactScale::of] is `None`, decoding one is `Error::SubtypeOutOfRange`).
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
pub struct ExactScale(Handle);

// 200 variants, so `Option<ExactScale>` and `Prefix` find a niche in the byte; made by index (`from_index`)
#[allow(dead_code)]
#[derive(Copy, Clone, PartialEq, Eq, Hash)]
#[repr(u8)]
enum Handle {
    I0,
    I1,
    I2,
    I3,
    I4,
    I5,
    I6,
    I7,
    I8,
    I9,
    I10,
    I11,
    I12,
    I13,
    I14,
    I15,
    I16,
    I17,
    I18,
    I19,
    I20,
    I21,
    I22,
    I23,
    I24,
    I25,
    I26,
    I27,
    I28,
    I29,
    I30,
    I31,
    I32,
    I33,
    I34,
    I35,
    I36,
    I37,
    I38,
    I39,
    I40,
    I41,
    I42,
    I43,
    I44,
    I45,
    I46,
    I47,
    I48,
    I49,
    I50,
    I51,
    I52,
    I53,
    I54,
    I55,
    I56,
    I57,
    I58,
    I59,
    I60,
    I61,
    I62,
    I63,
    I64,
    I65,
    I66,
    I67,
    I68,
    I69,
    I70,
    I71,
    I72,
    I73,
    I74,
    I75,
    I76,
    I77,
    I78,
    I79,
    I80,
    I81,
    I82,
    I83,
    I84,
    I85,
    I86,
    I87,
    I88,
    I89,
    I90,
    I91,
    I92,
    I93,
    I94,
    I95,
    I96,
    I97,
    I98,
    I99,
    I100,
    I101,
    I102,
    I103,
    I104,
    I105,
    I106,
    I107,
    I108,
    I109,
    I110,
    I111,
    I112,
    I113,
    I114,
    I115,
    I116,
    I117,
    I118,
    I119,
    I120,
    I121,
    I122,
    I123,
    I124,
    I125,
    I126,
    I127,
    I128,
    I129,
    I130,
    I131,
    I132,
    I133,
    I134,
    I135,
    I136,
    I137,
    I138,
    I139,
    I140,
    I141,
    I142,
    I143,
    I144,
    I145,
    I146,
    I147,
    I148,
    I149,
    I150,
    I151,
    I152,
    I153,
    I154,
    I155,
    I156,
    I157,
    I158,
    I159,
    I160,
    I161,
    I162,
    I163,
    I164,
    I165,
    I166,
    I167,
    I168,
    I169,
    I170,
    I171,
    I172,
    I173,
    I174,
    I175,
    I176,
    I177,
    I178,
    I179,
    I180,
    I181,
    I182,
    I183,
    I184,
    I185,
    I186,
    I187,
    I188,
    I189,
    I190,
    I191,
    I192,
    I193,
    I194,
    I195,
    I196,
    I197,
    I198,
    I199,
}

/// The primes of `2^b - 1` for `b` in 2..=32, a power repeated, back to back; [FULL_ENDS] marks where each ends.
const FULL_PRIMES: [u32; 97] = [
    3, // 2
    7, // 3
    3, 5,  // 4
    31, // 5
    3, 3, 7,   // 6
    127, // 7
    3, 5, 17, // 8
    7, 73, // 9
    3, 11, 31, // 10
    23, 89, // 11
    3, 3, 5, 7, 13,   // 12
    8191, // 13
    3, 43, 127, // 14
    7, 31, 151, // 15
    3, 5, 17, 257,    // 16
    131071, // 17
    3, 3, 3, 7, 19, 73,     // 18
    524287, // 19
    3, 5, 5, 11, 31, 41, // 20
    7, 7, 127, 337, // 21
    3, 23, 89, 683, // 22
    47, 178481, // 23
    3, 3, 5, 7, 13, 17, 241, // 24
    31, 601, 1801, // 25
    3, 2731, 8191, // 26
    7, 73, 262657, // 27
    3, 5, 29, 43, 113, 127, // 28
    233, 1103, 2089, // 29
    3, 3, 7, 11, 31, 151, 331,        // 30
    2147483647, // 31
    3, 5, 17, 257, 65537, // 32
];

/// The end of each `b`'s primes in [FULL_PRIMES], `b` = 2 first.
const FULL_ENDS: [u8; 31] = [
    1, 2, 4, 5, 8, 9, 12, 14, 17, 19, 24, 25, 28, 31, 35, 36, 42, 43, 49, 53, 57, 59, 66, 69, 72,
    75, 81, 84, 91, 92, 97,
];

/// The decimal exponents of the SI prefixes, which have no handle.
const fn is_si(e: i8) -> bool {
    matches!(e, -3..=3) || e % 3 == 0
}

impl ExactScale {
    /// The handle with this index, `None` past [EXACT_SCALES].
    pub(crate) const fn from_index(i: u8) -> Option<ExactScale> {
        if (i as usize) < EXACT_SCALES {
            // SAFETY: `Handle` is `repr(u8)` with the variants 0..EXACT_SCALES in order
            Some(ExactScale(unsafe { core::mem::transmute::<u8, Handle>(i) }))
        } else {
            None
        }
    }

    /// The handle's index, `0..EXACT_SCALES`.
    pub const fn index(self) -> u8 {
        self.0 as u8
    }

    /// The scale, exact.
    pub fn magnitude(self) -> Magnitude {
        let i = self.index();
        match Self::computed(i) {
            Some(m) => m,
            // an interned handle exists only once its slot is filled
            None => interner::get(i as usize - COMPUTED).unwrap_or(Magnitude::ONE),
        }
    }

    /// The scale of a computed handle, `None` for an interned one.
    pub const fn computed(i: u8) -> Option<Magnitude> {
        if i < POW2_START {
            // the k-th exponent in -30..=30 that isn't an SI prefix's
            let mut e = -30i8;
            let mut k = i - POW10_START;
            loop {
                if !is_si(e) {
                    if k == 0 {
                        return Some(Magnitude::pow10(e));
                    }
                    k -= 1;
                }
                e += 1;
            }
        } else if i < FULL_START {
            let mut e = -32i8 + (i - POW2_START) as i8;
            // 0, 10, 20 and 30 are 1, Ki, Mi and Gi
            if e >= 0 {
                e += 1;
            }
            if e >= 10 {
                e += 1;
            }
            if e >= 20 {
                e += 1;
            }
            if e >= 30 {
                e += 1;
            }
            Some(Magnitude::pow2(e))
        } else if i < PI_START {
            let b = (i - FULL_START) as usize;
            let start = if b == 0 { 0 } else { FULL_ENDS[b - 1] as usize };
            let (_, tail) = FULL_PRIMES.split_at(start);
            let (primes, _) = tail.split_at(FULL_ENDS[b] as usize - start);
            Some(Magnitude::recip_of_primes(primes))
        } else if (i as usize) < COMPUTED {
            let e = -32i8 + (i - PI_START) as i8;
            Some(Magnitude::PI.mul(Magnitude::pow2(e)))
        } else {
            None
        }
    }

    /// The computed handle of `m`, `None` when it has none.
    pub fn of_computed(m: &Magnitude) -> Option<ExactScale> {
        let (pow10_x2, pow2_x2) = m.pow10_pow2_x2();
        let irr = m.irrational_x2();
        let no_primes = m.primes().next().is_none();
        let i = if let Some(e) = m.as_pow10() {
            if is_si(e) || !(-30..=30).contains(&e) {
                return None;
            }
            // the exponents below e that aren't an SI prefix's
            (-30..e).filter(|e| !is_si(*e)).count() as u8 + POW10_START
        } else if pow10_x2 == 0
            && no_primes
            && pow2_x2 % 2 == 0
            && (irr.pi, irr.ln2, irr.ln10) == (0, 0, 0)
        {
            let e = pow2_x2 / 2;
            if !(-32..=32).contains(&e) || matches!(e, 0 | 10 | 20 | 30) {
                return None;
            }
            let skipped = [0, 10, 20, 30].iter().filter(|s| e > **s).count() as i8;
            (e + 32 - skipped) as u8 + POW2_START
        } else if pow10_x2 == 0
            && no_primes
            && pow2_x2 % 2 == 0
            && (irr.pi, irr.ln2, irr.ln10) == (2, 0, 0)
        {
            let e = pow2_x2 / 2;
            if !(-32..=1).contains(&e) {
                return None;
            }
            (e + 32) as u8 + PI_START
        } else {
            match m.to_ratio() {
                Some((1, den)) if (den + 1).is_power_of_two() => {
                    let b = (den + 1).trailing_zeros();
                    if !(2..=32).contains(&b) {
                        return None;
                    }
                    (b - 2) as u8 + FULL_START
                }
                _ => return None,
            }
        };
        Self::from_index(i)
    }

    /// The handle of `m`: a computed one, else (on `std`) interned. `None` for `m` that is a named prefix's scale
    /// ([Prefix::from_magnitude](crate::Prefix::from_magnitude) gives that one), without `std` for any `m` outside
    /// the computed scales, and on `std` once the interner is full.
    pub fn of(m: &Magnitude) -> Option<ExactScale> {
        if crate::Prefix::named_of(m).is_some() {
            return None;
        }
        match Self::of_computed(m) {
            Some(s) => Some(s),
            None => interner::intern(m).and_then(|k| Self::from_index((COMPUTED + k) as u8)),
        }
    }
}

#[cfg(feature = "std")]
mod interner {
    use super::{COMPUTED, EXACT_SCALES};
    use crate::Magnitude;
    use std::sync::Mutex;
    use std::vec::Vec;

    static INTERNED: Mutex<Vec<Magnitude>> = Mutex::new(Vec::new());

    fn lock() -> std::sync::MutexGuard<'static, Vec<Magnitude>> {
        // a panic elsewhere while holding the lock leaves the list whole: it is only pushed to
        INTERNED.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(super) fn get(k: usize) -> Option<Magnitude> {
        lock().get(k).copied()
    }

    pub(super) fn intern(m: &Magnitude) -> Option<usize> {
        let mut list = lock();
        if let Some(k) = list.iter().position(|x| x == m) {
            return Some(k);
        }
        if list.len() == EXACT_SCALES - COMPUTED {
            return None;
        }
        list.push(*m);
        Some(list.len() - 1)
    }
}

#[cfg(not(feature = "std"))]
mod interner {
    use crate::Magnitude;

    pub(super) fn get(_k: usize) -> Option<Magnitude> {
        None
    }

    pub(super) fn intern(_m: &Magnitude) -> Option<usize> {
        None
    }
}

impl core::fmt::Debug for ExactScale {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "ExactScale({})", self.magnitude())
    }
}

impl core::fmt::Display for ExactScale {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.magnitude(), f)
    }
}

#[cfg(feature = "defmt")]
impl defmt::Format for ExactScale {
    fn format(&self, f: defmt::Formatter) {
        defmt::write!(f, "{}", self.magnitude())
    }
}

#[cfg(feature = "shrink_wrap")]
mod wire {
    use super::ExactScale;
    use crate::Magnitude;
    use core::mem::MaybeUninit;
    use shrink_wrap::WireAs;
    use shrink_wrap::series::{SeriesCells, SeriesStore};
    use shrink_wrap::{Error, MaxSize};

    /// `#[wire(Magnitude)]` on [Prefix](crate::Prefix)'s `Exact`: the full magnitude on the wire.
    impl WireAs<ExactScale> for Magnitude {
        fn from_plain(value: ExactScale) -> Result<Self, Error> {
            Ok(value.magnitude())
        }

        fn into_plain(self) -> Result<ExactScale, Error> {
            ExactScale::of(&self).ok_or(Error::SubtypeOutOfRange)
        }

        /// The largest computed scale, 1/(2^30 − 1) (checked by a test), not a full `Magnitude`'s 97 B: firmware
        /// knows only the computed scales, and buffers and `Series::max_bytes` are sized by this. A scale interned on
        /// `std` may write more and then fails to fit a buffer sized by it ([Error::OutOfBounds]).
        const PLAIN_MAX_SIZE: Option<MaxSize> = Some(MaxSize::bytes(super::COMPUTED_MAX_BYTES));
    }

    /// A `Series` of prefixes keeps the handle's index.
    impl SeriesStore for ExactScale {
        type Store<const N: usize> = <u8 as SeriesStore>::Store<N>;

        fn empty_store<const N: usize>() -> Self::Store<N> {
            u8::empty_store::<N>()
        }

        fn init_store<const N: usize>(
            slot: &mut MaybeUninit<Self::Store<N>>,
        ) -> &mut Self::Store<N> {
            u8::init_store::<N>(slot)
        }

        #[inline(always)]
        fn set<const N: usize>(store: &mut Self::Store<N>, i: usize, item: &Self) {
            u8::set::<N>(store, i, &item.index());
        }

        fn get<const N: usize>(store: &Self::Store<N>, i: usize) -> Self {
            // only indices of handles are stored, an empty slot is 0, a handle too
            ExactScale::from_index(u8::get::<N>(store, i)).unwrap_or(ExactScale(super::Handle::I0))
        }
    }

    // SAFETY: the cells of `u8`, read back through `get`, which maps any byte to a handle
    unsafe impl SeriesCells for ExactScale {
        type Cells<const R: usize> = <u8 as SeriesCells>::Cells<R>;

        #[inline(always)]
        fn put<const R: usize>(cells: &Self::Cells<R>, i: usize, item: &Self) {
            u8::put::<R>(cells, i, &item.index());
        }

        fn copy_rows<const R: usize, const N: usize>(
            cells: &Self::Cells<R>,
            from: usize,
            store: &mut Self::Store<N>,
            to: usize,
            n: usize,
        ) {
            u8::copy_rows::<R, N>(cells, from, store, to, n);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_computed_handle_round_trips() {
        for i in 0..COMPUTED as u8 {
            let m = ExactScale::computed(i).unwrap();
            assert_eq!(
                ExactScale::of_computed(&m).map(|s| s.index()),
                Some(i),
                "{i}: {m}"
            );
            assert!(
                crate::Prefix::named_of(&m).is_none(),
                "{i}: {m} is a named prefix"
            );
        }
    }

    #[test]
    fn full_scales_are_two_to_the_b_minus_one() {
        for b in 2..=32u32 {
            let m = ExactScale::computed(FULL_START + (b - 2) as u8).unwrap();
            assert_eq!(m.to_ratio(), Some((1, (1u128 << b) - 1)), "b = {b}");
        }
        assert_eq!(
            ExactScale::of(&Magnitude::ratio(1, 65535)).map(|s| s.index()),
            Some(FULL_START + 14)
        );
    }

    #[test]
    fn named_scales_have_no_handle() {
        for m in [
            Magnitude::ONE,
            Magnitude::pow10(-3),
            Magnitude::pow2(10),
            Magnitude::pow2(40),
        ] {
            assert_eq!(ExactScale::of(&m), None, "{m}");
        }
    }

    #[test]
    fn other_scales_are_interned_once() {
        let m = Magnitude::ratio(7, 3);
        let a = ExactScale::of(&m).unwrap();
        assert!(a.index() as usize >= COMPUTED);
        assert_eq!(ExactScale::of(&m), Some(a));
        assert_eq!(a.magnitude(), m);
    }

    #[cfg(feature = "shrink_wrap")]
    #[test]
    fn computed_scales_fit_their_bound() {
        use shrink_wrap::SerializeShrinkWrap;
        let largest = (0..COMPUTED as u8)
            .map(|i| {
                let m = ExactScale::computed(i).unwrap();
                m.to_ww_bytes(&mut [0u8; 128]).unwrap().len()
            })
            .max();
        assert_eq!(largest, Some(COMPUTED_MAX_BYTES));
    }

    #[test]
    fn sizes() {
        assert_eq!(size_of::<ExactScale>(), 1);
        assert_eq!(size_of::<Option<ExactScale>>(), 1);
    }
}

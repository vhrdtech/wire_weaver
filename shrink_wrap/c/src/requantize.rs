//! The C requantize (`sw_requantize.h`, CLT-54) from Rust: [CFactor::of] works out a factor's constants on the
//! host from a [Magnitude], as a code generator does (and typed Rust at compile time), [CFactor::c_initializer]
//! is the C a generator emits for them, and [CRequantizer] runs the C conversion through FFI, for the cross-check
//! against `exact_units::Requantizer` in `tests/requantize.rs`.

use core::ffi::c_uint;
use exact_units::{Magnitude, Overflow, Repr, Rounding, StorageKind};

const SW_NUM_INT: u32 = 0;
const SW_NUM_UINT: u32 = 1;
const SW_NUM_F32: u32 = 2;
const SW_NUM_F64: u32 = 3;

/// `sw_factor`
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct CFactor {
    pub ratio: bool,
    pub num_hi: u64,
    pub num_lo: u64,
    pub den_hi: u64,
    pub den_lo: u64,
    pub m: u64,
    pub shift: i32,
    pub f_num: f64,
    pub f_den: f64,
}

/// `sw_storage`
#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct SwStorage {
    kind: u32,
    bits: c_uint,
}

/// `sw_num`, its union as the bits of a `u64`
#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct SwNum {
    kind: u32,
    v: u64,
}

/// `sw_requantizer`
#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct SwRequantizer {
    factor: CFactor,
    to: SwStorage,
    rounding: u32,
    overflow: u32,
}

unsafe extern "C" {
    fn sw_test_sizeof_factor() -> usize;
    fn sw_test_sizeof_requantizer() -> usize;
    fn sw_test_sizeof_num() -> usize;
    fn sw_requantize(q: *const SwRequantizer, x: SwNum, out: *mut SwNum) -> bool;
}

/// Sizes of `sw_factor`, `sw_requantizer` and `sw_num` on the C side and of their mirrors here.
pub fn c_struct_sizes() -> [(usize, usize); 3] {
    unsafe {
        [
            (sw_test_sizeof_factor(), size_of::<CFactor>()),
            (sw_test_sizeof_requantizer(), size_of::<SwRequantizer>()),
            (sw_test_sizeof_num(), size_of::<SwNum>()),
        ]
    }
}

impl CFactor {
    /// The constants of `factor`, as `exact_units::Requantizer::new` works them out: `to_ratio()` with `den`
    /// clamped to `i128::MAX` (it rounds the same), else `fixed_point()`; the float factor as
    /// `Magnitude::apply_f64` uses it.
    pub fn of(factor: &Magnitude) -> CFactor {
        let mut f = CFactor {
            ratio: false,
            num_hi: 0,
            num_lo: 0,
            den_hi: 0,
            den_lo: 0,
            m: 0,
            shift: 0,
            f_num: factor.to_f64(),
            f_den: 1.0,
        };
        match factor.to_ratio() {
            Some((num, den)) => {
                let den = den.min(i128::MAX as u128);
                f.ratio = true;
                (f.num_hi, f.num_lo) = ((num >> 64) as u64, num as u64);
                (f.den_hi, f.den_lo) = ((den >> 64) as u64, den as u64);
                if num <= 1 << 53 && den <= 1 << 53 {
                    (f.f_num, f.f_den) = (num as f64, den as f64);
                }
            }
            None => (f.m, f.shift) = factor.fixed_point(),
        }
        f
    }

    /// The C designated initializer of a `sw_factor` with these constants, doubles in their exact bits (C99 hex
    /// floats).
    pub fn c_initializer(&self) -> String {
        format!(
            "{{.ratio = {}, .num_hi = {}u, .num_lo = {}u, .den_hi = {}u, .den_lo = {}u, .m = {}u, .shift = {}, \
             .f_num = {}, .f_den = {}}}",
            self.ratio,
            hex_u64(self.num_hi),
            hex_u64(self.num_lo),
            hex_u64(self.den_hi),
            hex_u64(self.den_lo),
            hex_u64(self.m),
            self.shift,
            hex_f64(self.f_num),
            hex_f64(self.f_den),
        )
    }
}

fn hex_u64(x: u64) -> String {
    format!("UINT64_C({x:#x})")
}

/// A finite, positive double as a C99 hex float literal, exact.
pub fn hex_f64(x: f64) -> String {
    assert!(
        x.is_finite() && x > 0.0,
        "a factor is finite and positive: {x}"
    );
    let bits = x.to_bits();
    let exp = ((bits >> 52) & 0x7ff) as i32;
    let mant = bits & ((1 << 52) - 1);
    if exp == 0 {
        format!("0x0.{mant:013x}p-1022")
    } else {
        format!("0x1.{mant:013x}p{}", exp - 1023)
    }
}

/// The C `sw_rounding` value of a rounding.
pub fn c_rounding(r: Rounding) -> &'static str {
    match r {
        Rounding::Truncate => "SW_ROUND_TRUNCATE",
        Rounding::Floor => "SW_ROUND_FLOOR",
        Rounding::Ceil => "SW_ROUND_CEIL",
        Rounding::NearestEven => "SW_ROUND_NEAREST_EVEN",
        Rounding::NearestAway => "SW_ROUND_NEAREST_AWAY",
    }
}

/// The C `sw_overflow` value of an overflow policy.
pub fn c_overflow(o: Overflow) -> &'static str {
    match o {
        Overflow::Checked => "SW_OVERFLOW_CHECKED",
        Overflow::Saturate => "SW_OVERFLOW_SATURATE",
        Overflow::Wrap => "SW_OVERFLOW_WRAP",
    }
}

/// The C `sw_storage` initializer of a storage.
pub fn c_storage(to: StorageKind) -> String {
    match to {
        StorageKind::Int { bits, signed } => {
            format!(
                "{{{}, {bits}}}",
                if signed { "SW_NUM_INT" } else { "SW_NUM_UINT" }
            )
        }
        StorageKind::F32 => "{SW_NUM_F32, 32}".into(),
        StorageKind::F64 => "{SW_NUM_F64, 64}".into(),
    }
}

/// The C conversion, `sw_requantize`, through FFI.
#[derive(Copy, Clone, Debug)]
pub struct CRequantizer {
    raw: SwRequantizer,
}

impl CRequantizer {
    pub fn new(factor: &Magnitude, to: StorageKind, r: Rounding, o: Overflow) -> Self {
        let (kind, bits) = match to {
            StorageKind::Int { bits, signed } => {
                (if signed { SW_NUM_INT } else { SW_NUM_UINT }, bits)
            }
            StorageKind::F32 => (SW_NUM_F32, 32),
            StorageKind::F64 => (SW_NUM_F64, 64),
        };
        let rounding = match r {
            Rounding::Truncate => 0,
            Rounding::Floor => 1,
            Rounding::Ceil => 2,
            Rounding::NearestEven => 3,
            Rounding::NearestAway => 4,
        };
        let overflow = match o {
            Overflow::Checked => 0,
            Overflow::Saturate => 1,
            Overflow::Wrap => 2,
        };
        CRequantizer {
            raw: SwRequantizer {
                factor: CFactor::of(factor),
                to: SwStorage { kind, bits },
                rounding,
                overflow,
            },
        }
    }

    /// `x` converted by the C side; an integer `x` of more than 64 bits panics (C takes up to 64).
    pub fn apply(&self, x: Repr) -> Option<Repr> {
        let x = match x {
            Repr::Int(i) if i < 0 => SwNum {
                kind: SW_NUM_INT,
                v: i64::try_from(i).expect("C takes integers of up to 64 bits") as u64,
            },
            Repr::Int(i) => SwNum {
                kind: SW_NUM_UINT,
                v: u64::try_from(i).expect("C takes integers of up to 64 bits"),
            },
            Repr::Float(f) => SwNum {
                kind: SW_NUM_F64,
                v: f.to_bits(),
            },
        };
        let mut out = SwNum { kind: 0, v: 0 };
        if !unsafe { sw_requantize(&self.raw, x, &mut out) } {
            return None;
        }
        assert_eq!(
            out.kind, self.raw.to.kind,
            "sw_requantize gave another kind"
        );
        Some(match out.kind {
            SW_NUM_INT => Repr::Int(out.v as i64 as i128),
            SW_NUM_UINT => Repr::Int(out.v as i128),
            _ => Repr::Float(f64::from_bits(out.v)),
        })
    }
}

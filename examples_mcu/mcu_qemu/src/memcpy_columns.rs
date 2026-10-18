//! Upper bound of an aligned-plain `Series` column (experiment series-memcpy-fast, 18 Oct 2026): one FOC level batch
//! (the 21 `Foc` properties of the motor demo, [super::foc_signals], n = 32) plus its clock, copied from
//! struct-of-arrays storage into a buffer as byte-aligned little-endian values, with no per-item field walk and no
//! bit writer. Compare with the `shared_clock` bench (21 x `Series<T>` + `Series<Second>`, `DeltaPacked` and
//! `RawPlain`). Used by `qemu-memcpy-columns` (instructions, `qemu_icount`) and `stm32h725ig-memcpy-columns-cycles`
//! (DWT cycles), through [Meter].
//!
//! The store mirrors what `SeriesStore` keeps for a `ww_si` quantity (`Quantity { prefix, value: NumericValue }`):
//! an array of `Prefix` discriminants, an array of `NumericValue` variant bytes and one array of 4-byte payloads.
//! Two shapes are measured:
//! - [Shape::Bare]: the 22 payload arrays copied back to back, nothing else: the floor.
//! - [Shape::Framed]: what a real column needs on top: the item count once, and per column a check that its prefix
//!   and variant are the same for every item (else the column could not be written this way and would fall back to
//!   the item walk), those two bytes, then the payloads.

use super::foc_signals as sig;

/// Samples per property per batch.
pub const N: usize = 32;
/// Batches in the recorded signals.
pub const BATCHES: usize = 2;
/// 21 properties and the clock.
pub const COLUMNS: usize = 22;
/// Room for the biggest shape.
pub const BUF: usize = COLUMNS * (2 + 4 * N) + 4;

/// A quantity column as `SeriesStore` keeps it.
#[derive(Clone, Copy)]
pub struct QuantityColumn {
    pub prefix: [u8; N],
    pub kind: [u8; N],
    pub payload: [u32; N],
}

/// `NumericValue::I32` and `U32` variant bytes, `Prefix::Milli`, `Micro`, `Nano` discriminants.
const I32: u8 = 6;
const U32: u8 = 3;
const MILLI: u8 = 1;
const MICRO: u8 = 2;
const NANO: u8 = 3;

/// The level's store for one batch: the clock first, then the 21 properties in API order.
pub fn level(batch: usize) -> [QuantityColumn; COLUMNS] {
    let col = |prefix: u8, kind: u8, f: &dyn Fn(usize) -> u32| QuantityColumn {
        prefix: [prefix; N],
        kind: [kind; N],
        payload: core::array::from_fn(f),
    };
    let s = |p: u8, a: &[i32; 64]| col(p, I32, &|i| a[batch * N + i] as u32);
    [
        col(MICRO, U32, &|i| {
            sig::T0_US + (batch * N + i) as u32 * sig::DT_US
        }),
        s(MILLI, &sig::I_A),
        s(MILLI, &sig::I_B),
        s(MILLI, &sig::I_C),
        s(MILLI, &sig::I_ALPHA),
        s(MILLI, &sig::I_BETA),
        s(MILLI, &sig::I_D),
        s(MILLI, &sig::I_Q),
        s(MILLI, &sig::V_D),
        s(MILLI, &sig::V_Q),
        s(MILLI, &sig::V_ALPHA),
        s(MILLI, &sig::V_BETA),
        s(MICRO, &sig::DUTY_A),
        s(MICRO, &sig::DUTY_B),
        s(MICRO, &sig::DUTY_C),
        s(MICRO, &sig::PHASE),
        s(MICRO, &sig::PHASE_OBSERVER),
        s(MICRO, &sig::PHASE_ENCODER),
        s(NANO, &sig::OBSERVER_X1),
        s(NANO, &sig::OBSERVER_X2),
        s(MILLI, &sig::V_BUS),
        s(MICRO, &sig::PHASE_TRUE),
    ]
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    Bare,
    Framed,
}

impl Shape {
    pub const ALL: [Shape; 2] = [Shape::Bare, Shape::Framed];

    pub fn label(self) -> &'static str {
        match self {
            Shape::Bare => "22 payload arrays copied",
            Shape::Framed => "22 aligned-plain columns",
        }
    }
}

/// Runs one measured piece of work and reports it: `work` returns the bytes it wrote.
pub trait Meter {
    fn measure(&mut self, shape: Shape, batch: usize, work: &mut dyn FnMut() -> usize);
}

/// Payloads as little-endian bytes: a plain copy on a little-endian target.
#[inline(always)]
fn put_payload(dst: &mut [u8], src: &[u32; N]) {
    let dst = &mut dst[..4 * N];
    #[cfg(target_endian = "little")]
    {
        // SAFETY: `[u32; N]` is `4 * N` initialized bytes, `u8` has no alignment.
        let bytes = unsafe { core::slice::from_raw_parts(src.as_ptr().cast::<u8>(), 4 * N) };
        dst.copy_from_slice(bytes);
    }
    #[cfg(not(target_endian = "little"))]
    for (d, v) in dst.chunks_exact_mut(4).zip(src) {
        d.copy_from_slice(&v.to_le_bytes());
    }
}

#[inline(never)]
pub fn bare(store: &[QuantityColumn; COLUMNS], buf: &mut [u8]) -> usize {
    let mut at = 0;
    for c in store {
        put_payload(&mut buf[at..], &c.payload);
        at += 4 * N;
    }
    at
}

/// The byte every item has, compared four at a time.
fn same(a: &[u8; N]) -> Option<u8> {
    let first = a[0];
    let word = u32::from_ne_bytes([first; 4]);
    a.chunks_exact(4)
        .all(|w| u32::from_ne_bytes([w[0], w[1], w[2], w[3]]) == word)
        .then_some(first)
}

/// `None` when a column's prefix or variant changes within the batch.
#[inline(never)]
pub fn framed(store: &[QuantityColumn; COLUMNS], buf: &mut [u8]) -> Option<usize> {
    buf[0] = N as u8; // item count, one UVlq byte up to 127
    let mut at = 1;
    for c in store {
        buf[at] = same(&c.prefix)?;
        buf[at + 1] = same(&c.kind)?;
        at += 2;
        put_payload(&mut buf[at..], &c.payload);
        at += 4 * N;
    }
    Some(at)
}

pub fn run(meter: &mut impl Meter) {
    let mut buf = [0u8; BUF];
    for batch in 0..BATCHES {
        let store = level(batch);
        let store = core::hint::black_box(&store);
        meter.measure(Shape::Bare, batch, &mut || {
            let n = bare(store, core::hint::black_box(&mut buf));
            core::hint::black_box(&buf);
            n
        });
        meter.measure(Shape::Framed, batch, &mut || {
            let n = framed(store, core::hint::black_box(&mut buf)).unwrap();
            core::hint::black_box(&buf);
            n
        });
    }
}

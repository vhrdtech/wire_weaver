//! The NEON group kernel: 8 values per step as two u32x4.
//!
//! 8 values take exactly w bytes, so step s starts byte aligned at s w and one table per width serves all 16 steps:
//! a `tbl` gathers the 4 bytes from a value's first byte into its lane, a variable shift drops the bits before it,
//! and for values that span 5 bytes (some lane when w is 26 or more) a second `tbl` brings in the fifth byte.
#![expect(
    unsafe_code,
    reason = "core::arch loads, stores and calls into target_feature functions"
)]
#![warn(clippy::undocumented_unsafe_blocks)]

use super::{GROUP, mask32};
use core::arch::aarch64::*;

/// Values per step.
const STEP: usize = 8;
const STEPS: usize = GROUP / STEP;
/// Bytes one step loads: 8 values at width 32.
const WINDOW: usize = 32;
/// `tbl` index that yields 0.
const NONE: u8 = 0xff;

/// Bytes the kernel reads from a group's start: step 15's window at 15 w.
pub(super) const fn reach(w: u32) -> usize {
    (STEPS - 1) * w as usize + WINDOW
}

/// `tbl` indices and shifts that turn one step's window into two u32x4.
#[derive(Clone, Copy)]
struct Table {
    /// Bytes b..b+4 of each value, lanes 0..4 and 4..8.
    lo: [[u8; 16]; 2],
    /// Bytes b+1..b+5 of each value.
    hi: [[u8; 16]; 2],
    /// Right shift of `lo` by the value's bit in its first byte, as a negative left shift.
    lo_sh: [[i32; 4]; 2],
    /// Left shift of `hi`: 8 less that bit.
    hi_sh: [[i32; 4]; 2],
    /// Some value spans 5 bytes, so `hi` is needed.
    hi_needed: bool,
}

const fn window_index(b: usize) -> u8 {
    if b < WINDOW { b as u8 } else { NONE }
}

const fn table(w: usize) -> Table {
    let mut t = Table {
        lo: [[NONE; 16]; 2],
        hi: [[NONE; 16]; 2],
        lo_sh: [[0; 4]; 2],
        hi_sh: [[0; 4]; 2],
        hi_needed: false,
    };
    let mut i = 0;
    while i < STEP {
        let (b, s) = (i * w / 8, i * w % 8);
        let (reg, lane) = (i / 4, i % 4);
        let mut k = 0;
        while k < 4 {
            t.lo[reg][lane * 4 + k] = window_index(b + k);
            t.hi[reg][lane * 4 + k] = window_index(b + 1 + k);
            k += 1;
        }
        t.lo_sh[reg][lane] = -(s as i32);
        t.hi_sh[reg][lane] = 8 - s as i32;
        if s + w > 32 {
            t.hi_needed = true;
        }
        i += 1;
    }
    t
}

/// `TABLES[w]` for widths 0..=32.
static TABLES: [Table; 33] = {
    let mut t = [table(0); 33];
    let mut w = 1;
    while w <= 32 {
        t[w] = table(w);
        w += 1;
    }
    t
};

/// Unpack a group at width `w` (1..=32).
///
/// # Panics
///
/// When `w` is outside 1..=32 or `src` is shorter than [reach].
pub(super) fn unpack(src: &[u8], w: u32, out: &mut [u32; GROUP]) {
    assert!((1..=32).contains(&w), "width {w} outside 1..=32");
    assert!(
        src.len() >= reach(w),
        "group shorter than the kernel's reach"
    );
    // SAFETY: this module is built only with target_feature = "neon", so the CPU running it has NEON, and src
    // holds reach(w) bytes.
    unsafe { steps(src, w, |s, a, b| store8(out, s, a, b)) }
}

#[target_feature(enable = "neon")]
#[inline]
fn store8(out: &mut [u32; GROUP], s: usize, a: uint32x4_t, b: uint32x4_t) {
    let o = &mut out[s * STEP..(s + 1) * STEP];
    // SAFETY: o is 8 writable u32; vst1q_u32_x2 writes 8.
    unsafe { vst1q_u32_x2(o.as_mut_ptr(), uint32x4x2_t(a, b)) }
}

/// Decode a group at width `w` (1..=32) to `base + line(slope, j) + value[j]`.
///
/// # Panics
///
/// As [unpack].
pub(super) fn decode(src: &[u8], w: u32, base: i64, slope: i64, out: &mut [i64; GROUP]) {
    assert!((1..=32).contains(&w), "width {w} outside 1..=32");
    assert!(
        src.len() >= reach(w),
        "group shorter than the kernel's reach"
    );
    // SAFETY: as in unpack.
    unsafe {
        let mut line = Line::new(base, slope);
        steps(src, w, |s, a, b| line.put(out, s, a, b));
    }
}

#[target_feature(enable = "neon")]
#[inline]
fn load_u8x16(b: &[u8; 16]) -> uint8x16_t {
    // SAFETY: b is 16 readable bytes; vld1q_u8 reads 16 and has no alignment requirement.
    unsafe { vld1q_u8(b.as_ptr()) }
}

#[target_feature(enable = "neon")]
#[inline]
fn load_i32x4(v: &[i32; 4]) -> int32x4_t {
    // SAFETY: v is 4 readable i32; vld1q_s32 reads 4 and needs only i32 alignment.
    unsafe { vld1q_s32(v.as_ptr()) }
}

/// Calls `f(step, lanes 0..4, lanes 4..8)` for the 16 steps of a group.
///
/// # Safety
///
/// `src` holds [reach]`(w)` bytes.
#[target_feature(enable = "neon")]
#[inline]
unsafe fn steps(src: &[u8], w: u32, f: impl FnMut(usize, uint32x4_t, uint32x4_t)) {
    let t = &TABLES[w as usize];
    // SAFETY: the caller's contract.
    unsafe {
        if t.hi_needed {
            steps_with::<true>(src, w, t, f)
        } else {
            steps_with::<false>(src, w, t, f)
        }
    }
}

/// # Safety
///
/// `src` holds [reach]`(w)` bytes.
#[target_feature(enable = "neon")]
#[inline]
unsafe fn steps_with<const HI: bool>(
    src: &[u8],
    w: u32,
    t: &Table,
    mut f: impl FnMut(usize, uint32x4_t, uint32x4_t),
) {
    let m = vdupq_n_u32(mask32(w));
    let (lo0, lo1) = (load_u8x16(&t.lo[0]), load_u8x16(&t.lo[1]));
    let (hi0, hi1) = (load_u8x16(&t.hi[0]), load_u8x16(&t.hi[1]));
    let (ls0, ls1) = (load_i32x4(&t.lo_sh[0]), load_i32x4(&t.lo_sh[1]));
    let (hs0, hs1) = (load_i32x4(&t.hi_sh[0]), load_i32x4(&t.hi_sh[1]));
    let lane = |win: uint8x16x2_t, lo, ls, hi, hs| {
        let mut v = vshlq_u32(vreinterpretq_u32_u8(vqtbl2q_u8(win, lo)), ls);
        if HI {
            v = vorrq_u32(v, vshlq_u32(vreinterpretq_u32_u8(vqtbl2q_u8(win, hi)), hs));
        }
        vandq_u32(v, m)
    };
    for s in 0..STEPS {
        let win = &src[s * w as usize..s * w as usize + WINDOW];
        // SAFETY: win is WINDOW = 32 readable bytes; vld1q_u8_x2 reads 32 with no alignment requirement.
        let win = unsafe { vld1q_u8_x2(win.as_ptr()) };
        f(
            s,
            lane(win, lo0, ls0, hi0, hs0),
            lane(win, lo1, ls1, hi1, hs1),
        );
    }
}

/// The group's line as a running sum: lane pairs hold the slope sums of 8 consecutive values.
struct Line {
    base: int64x2_t,
    p: [int64x2_t; 4],
    inc: int64x2_t,
}

impl Line {
    #[target_feature(enable = "neon")]
    #[inline]
    fn new(base: i64, slope: i64) -> Self {
        let pair = |j: i64| {
            let v = [j.wrapping_mul(slope), (j + 1).wrapping_mul(slope)];
            // SAFETY: v is 2 readable i64; vld1q_s64 reads 2.
            unsafe { vld1q_s64(v.as_ptr()) }
        };
        Self {
            base: vdupq_n_s64(base),
            p: [pair(0), pair(2), pair(4), pair(6)],
            inc: vdupq_n_s64(slope.wrapping_mul(8)),
        }
    }

    /// Writes base + (p >> 16) + r for the 8 values of step `s`.
    #[target_feature(enable = "neon")]
    #[inline]
    fn put(&mut self, out: &mut [i64; GROUP], s: usize, a: uint32x4_t, b: uint32x4_t) {
        let at = |p| vreinterpretq_u64_s64(vsraq_n_s64::<16>(self.base, p));
        let o = int64x2x4_t(
            vreinterpretq_s64_u64(vaddw_u32(at(self.p[0]), vget_low_u32(a))),
            vreinterpretq_s64_u64(vaddw_high_u32(at(self.p[1]), a)),
            vreinterpretq_s64_u64(vaddw_u32(at(self.p[2]), vget_low_u32(b))),
            vreinterpretq_s64_u64(vaddw_high_u32(at(self.p[3]), b)),
        );
        for p in &mut self.p {
            *p = vaddq_s64(*p, self.inc);
        }
        let dst = &mut out[s * STEP..(s + 1) * STEP];
        // SAFETY: dst is 8 writable i64; vst1q_s64_x4 writes 8.
        unsafe { vst1q_s64_x4(dst.as_mut_ptr(), o) }
    }
}

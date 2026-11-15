//! The AVX2 group kernel: 8 values per 256-bit register, gathered with byte shuffles and shifted per lane.
//!
//! A block of 8 values is 8 w bits, so block b starts on byte b w. Its low 128-bit half holds values 0..4 from byte
//! b w, its high half values 4..8 from byte b w + w / 2 (a nibble further at odd widths), each within 16 bytes. Byte
//! t of lane i takes byte `first + t` (`lo`) and `first + 1 + t` (`hi`, only for widths over 25, where a value can
//! span 5 bytes) of its half; an index past the half is 0x80, which the shuffle turns into 0.
#![expect(
    unsafe_code,
    reason = "core::arch loads, stores and calls into target_feature functions"
)]
#![warn(clippy::undocumented_unsafe_blocks)]

use super::{GROUP, mask32};
use core::arch::x86_64::*;

/// Blocks of 8 values in a group.
const BLOCKS: usize = GROUP / 8;
/// Widest value whose 4 gathered bytes always hold it: its bit offset in its first byte (at most 7) plus the width
/// is at most 32. Wider ones gather a fifth byte with a second shuffle.
const WIDTH_ONE_SHUFFLE_MAX: u32 = 25;

/// Bytes the kernel reads from a group's start: block 15's high half, 16 bytes at 15 w + w / 2.
pub(super) const fn reach(w: u32) -> usize {
    let w = w as usize;
    (BLOCKS - 1) * w + w / 2 + 16
}

/// Shuffle and shift tables, one row per width 0..=32.
struct Tables {
    lo: [[u8; 32]; 33],
    hi: [[u8; 32]; 33],
    /// Bit offset of each lane's value in its first byte.
    sh: [[u32; 8]; 33],
    /// 8 minus `sh`: where the `hi` bytes go.
    sh_hi: [[u32; 8]; 33],
}

const fn tables() -> Tables {
    let mut t = Tables {
        lo: [[0; 32]; 33],
        hi: [[0; 32]; 33],
        sh: [[0; 8]; 33],
        sh_hi: [[0; 8]; 33],
    };
    let mut w = 0;
    while w <= 32 {
        let mut i = 0;
        while i < 8 {
            let rel = (i % 4) * w + if i >= 4 { (4 * w) % 8 } else { 0 };
            let first = rel / 8;
            t.sh[w][i] = (rel % 8) as u32;
            t.sh_hi[w][i] = 8 - (rel % 8) as u32;
            let mut b = 0;
            while b < 4 {
                let byte = 4 * (i % 4) + b + 16 * (i / 4);
                t.lo[w][byte] = if first + b < 16 {
                    (first + b) as u8
                } else {
                    0x80
                };
                t.hi[w][byte] = if first + 1 + b < 16 {
                    (first + 1 + b) as u8
                } else {
                    0x80
                };
                b += 1;
            }
            i += 1;
        }
        w += 1;
    }
    t
}

static TABLES: Tables = tables();

/// Unpack a group at width `w` (1..=32).
///
/// # Panics
///
/// When `w` is outside 1..=32, `src` is shorter than [reach] or the CPU lacks AVX2.
pub(super) fn unpack(src: &[u8], w: u32, out: &mut [u32; GROUP]) {
    assert!((1..=32).contains(&w), "width {w} outside 1..=32");
    assert!(
        src.len() >= reach(w),
        "group shorter than the kernel's reach"
    );
    assert!(super::has_avx2(), "AVX2 kernel without AVX2");
    // SAFETY: the CPU has AVX2 (checked above), and src holds reach(w) bytes, every byte the kernel loads.
    unsafe {
        if w <= WIDTH_ONE_SHUFFLE_MAX {
            unpack_k::<false>(src, w, out)
        } else {
            unpack_k::<true>(src, w, out)
        }
    }
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
    assert!(super::has_avx2(), "AVX2 kernel without AVX2");
    // SAFETY: as in unpack.
    unsafe {
        if w <= WIDTH_ONE_SHUFFLE_MAX {
            decode_k::<false>(src, w, base, slope, out)
        } else {
            decode_k::<true>(src, w, base, slope, out)
        }
    }
}

/// Per-width registers: shuffle indices, shifts and the value mask.
#[derive(Clone, Copy)]
struct Ctx {
    w: usize,
    lo: __m256i,
    hi: __m256i,
    sh: __m256i,
    sh_hi: __m256i,
    m: __m256i,
}

#[target_feature(enable = "avx2")]
#[inline]
fn ctx(w: u32) -> Ctx {
    let w = w as usize;
    // SAFETY: each row is 32 bytes, one full 256-bit load; loadu has no alignment requirement.
    unsafe {
        Ctx {
            w,
            lo: _mm256_loadu_si256(TABLES.lo[w].as_ptr().cast()),
            hi: _mm256_loadu_si256(TABLES.hi[w].as_ptr().cast()),
            sh: _mm256_loadu_si256(TABLES.sh[w].as_ptr().cast()),
            sh_hi: _mm256_loadu_si256(TABLES.sh_hi[w].as_ptr().cast()),
            m: _mm256_set1_epi32(mask32(w as u32) as i32),
        }
    }
}

/// Values 8 b .. 8 b + 8. `WIDE` adds the fifth byte a value over 25 bits can need.
///
/// # Safety
///
/// `src` holds [reach]`(w)` bytes.
#[target_feature(enable = "avx2")]
#[inline]
unsafe fn block<const WIDE: bool>(src: &[u8], b: usize, c: &Ctx) -> __m256i {
    debug_assert!(b < BLOCKS, "block {b} out of range");
    let p = src.as_ptr();
    // SAFETY: b w + w / 2 + 16 <= reach(w) <= src.len() (the caller's contract); loadu has no alignment requirement.
    let x = unsafe { _mm256_loadu2_m128i(p.add(b * c.w + c.w / 2).cast(), p.add(b * c.w).cast()) };
    let lo = _mm256_srlv_epi32(_mm256_shuffle_epi8(x, c.lo), c.sh);
    if WIDE {
        let hi = _mm256_sllv_epi32(_mm256_shuffle_epi8(x, c.hi), c.sh_hi);
        _mm256_and_si256(_mm256_or_si256(lo, hi), c.m)
    } else {
        _mm256_and_si256(lo, c.m)
    }
}

/// # Safety
///
/// `src` holds [reach]`(w)` bytes.
#[target_feature(enable = "avx2")]
unsafe fn unpack_k<const WIDE: bool>(src: &[u8], w: u32, out: &mut [u32; GROUP]) {
    let c = ctx(w);
    for b in 0..BLOCKS {
        // SAFETY: the caller's contract is block's.
        let v = unsafe { block::<WIDE>(src, b, &c) };
        // SAFETY: b < BLOCKS, so words 8 b .. 8 b + 8 lie inside out; storeu has no alignment requirement.
        unsafe { _mm256_storeu_si256(out.as_mut_ptr().add(8 * b).cast(), v) };
    }
}

/// # Safety
///
/// `src` holds [reach]`(w)` bytes.
#[target_feature(enable = "avx2")]
unsafe fn decode_k<const WIDE: bool>(
    src: &[u8],
    w: u32,
    base: i64,
    slope: i64,
    out: &mut [i64; GROUP],
) {
    let c = ctx(w);
    // AVX2 has no 64-bit arithmetic shift: p holds (8 b + l) slope + 2^63, whose logical shift by 16 is
    // (p >> 16) + 2^47, and the base has 2^47 taken off.
    let s = |k: i64| slope.wrapping_mul(k).wrapping_add(i64::MIN);
    let base = _mm256_set1_epi64x(base.wrapping_sub(1 << 47));
    let mut p0 = _mm256_setr_epi64x(s(0), s(1), s(2), s(3));
    let mut p1 = _mm256_setr_epi64x(s(4), s(5), s(6), s(7));
    let step = _mm256_set1_epi64x(slope.wrapping_mul(8));
    let o = out.as_mut_ptr();
    for b in 0..BLOCKS {
        // SAFETY: the caller's contract is block's.
        let v = unsafe { block::<WIDE>(src, b, &c) };
        let r0 = _mm256_cvtepu32_epi64(_mm256_castsi256_si128(v));
        let r1 = _mm256_cvtepu32_epi64(_mm256_extracti128_si256::<1>(v));
        let t0 = _mm256_add_epi64(_mm256_add_epi64(base, _mm256_srli_epi64::<16>(p0)), r0);
        let t1 = _mm256_add_epi64(_mm256_add_epi64(base, _mm256_srli_epi64::<16>(p1)), r1);
        p0 = _mm256_add_epi64(p0, step);
        p1 = _mm256_add_epi64(p1, step);
        // SAFETY: b < BLOCKS, so i64s 8 b .. 8 b + 8 lie inside out; storeu has no alignment requirement.
        unsafe {
            _mm256_storeu_si256(o.add(8 * b).cast(), t0);
            _mm256_storeu_si256(o.add(8 * b + 4).cast(), t1);
        }
    }
}

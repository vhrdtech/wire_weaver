//! The AVX-512 group kernel: 16 values per 512-bit register, gathered with a VBMI byte permute and shifted with a
//! VBMI2 funnel shift.
//!
//! 16 values are 2 w bytes, so block b starts byte aligned at 2 b w. Lane i takes the 4 bytes from its value's first
//! byte (`lo`) and the 4 after them (`hi`); the funnel shift by the value's bit offset in its first byte joins them.
#![expect(
    unsafe_code,
    reason = "core::arch loads, stores and calls into target_feature functions"
)]
#![warn(clippy::undocumented_unsafe_blocks)]

use super::{GROUP, mask32};
use core::arch::x86_64::*;

/// Blocks of 16 values in a group.
const BLOCKS: usize = GROUP / 16;

/// Bytes the kernel reads from a group's start: block 7's 64-byte load at 14 w.
pub(super) const fn reach(w: u32) -> usize {
    2 * (BLOCKS - 1) * w as usize + 64
}

/// Unpack a group at width `w` (1..=32).
///
/// # Panics
///
/// When `w` is outside 1..=32, `src` is shorter than [reach] or the CPU lacks the AVX-512 features.
pub(super) fn unpack(src: &[u8], w: u32, out: &mut [u32; GROUP]) {
    assert!((1..=32).contains(&w), "width {w} outside 1..=32");
    assert!(
        src.len() >= reach(w),
        "group shorter than the kernel's reach"
    );
    assert!(super::has_avx512(), "AVX-512 kernel without AVX-512");
    // SAFETY: the CPU has the features (checked above), and src holds reach(w) bytes, every byte the kernel loads.
    unsafe { unpack_k(src, w, out) }
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
    assert!(super::has_avx512(), "AVX-512 kernel without AVX-512");
    // SAFETY: as in unpack.
    unsafe { decode_k(src, w, base, slope, out) }
}

/// Per-width registers: permute indices, shifts and the value mask.
#[derive(Clone, Copy)]
struct Ctx {
    w: usize,
    lo: __m512i,
    hi: __m512i,
    sh: __m512i,
    m: __m512i,
}

#[target_feature(enable = "avx512f")]
#[inline]
fn ctx(w: u32) -> Ctx {
    let iw = _mm512_mullo_epi32(
        _mm512_setr_epi32(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15),
        _mm512_set1_epi32(w as i32),
    );
    // first byte of value i, copied into the 4 bytes of its lane, plus 0..3 (lo) and 4..7 (hi)
    let first = _mm512_mullo_epi32(_mm512_srli_epi32::<3>(iw), _mm512_set1_epi32(0x0101_0101));
    // bytes past 2 w hold no bits of the block, so an index wrapped to 0..63 only fills discarded bits
    let wrap = _mm512_set1_epi32(0x3f3f_3f3f);
    Ctx {
        w: w as usize,
        lo: _mm512_and_si512(
            _mm512_add_epi32(first, _mm512_set1_epi32(0x0302_0100)),
            wrap,
        ),
        hi: _mm512_and_si512(
            _mm512_add_epi32(first, _mm512_set1_epi32(0x0706_0504)),
            wrap,
        ),
        sh: _mm512_and_si512(iw, _mm512_set1_epi32(7)),
        m: _mm512_set1_epi32(mask32(w) as i32),
    }
}

/// Values 16 b .. 16 b + 16.
///
/// # Safety
///
/// `src` holds [reach]`(w)` bytes.
#[target_feature(enable = "avx512f,avx512bw,avx512vbmi,avx512vbmi2")]
#[inline]
unsafe fn block(src: &[u8], b: usize, c: &Ctx) -> __m512i {
    debug_assert!(b < BLOCKS, "block {b} out of range");
    // SAFETY: 2 b w + 64 <= reach(w) <= src.len() (the caller's contract); loadu has no alignment requirement.
    let x = unsafe { _mm512_loadu_si512(src.as_ptr().add(2 * b * c.w).cast()) };
    let lo = _mm512_permutexvar_epi8(c.lo, x);
    let hi = _mm512_permutexvar_epi8(c.hi, x);
    _mm512_and_si512(_mm512_shrdv_epi32(lo, hi, c.sh), c.m)
}

/// # Safety
///
/// `src` holds [reach]`(w)` bytes.
#[target_feature(enable = "avx512f,avx512bw,avx512vbmi,avx512vbmi2")]
unsafe fn unpack_k(src: &[u8], w: u32, out: &mut [u32; GROUP]) {
    let c = ctx(w);
    for b in 0..BLOCKS {
        // SAFETY: the caller's contract is block's.
        let v = unsafe { block(src, b, &c) };
        // SAFETY: b < BLOCKS, so words 16 b .. 16 b + 16 lie inside out; storeu has no alignment requirement.
        unsafe { _mm512_storeu_si512(out.as_mut_ptr().add(16 * b).cast(), v) };
    }
}

/// # Safety
///
/// `src` holds [reach]`(w)` bytes.
#[target_feature(enable = "avx512f,avx512bw,avx512vbmi,avx512vbmi2")]
unsafe fn decode_k(src: &[u8], w: u32, base: i64, slope: i64, out: &mut [i64; GROUP]) {
    let c = ctx(w);
    let s = |k: i64| slope.wrapping_mul(k);
    let base = _mm512_set1_epi64(base);
    // lane l holds (8 k + l) slope: a running sum, no 64-bit multiply
    let mut p = _mm512_setr_epi64(0, s(1), s(2), s(3), s(4), s(5), s(6), s(7));
    let step = _mm512_set1_epi64(s(8));
    let o = out.as_mut_ptr();
    for b in 0..BLOCKS {
        // SAFETY: the caller's contract is block's.
        let v = unsafe { block(src, b, &c) };
        let r0 = _mm512_cvtepu32_epi64(_mm512_castsi512_si256(v));
        let r1 = _mm512_cvtepu32_epi64(_mm512_extracti64x4_epi64::<1>(v));
        let t0 = _mm512_add_epi64(_mm512_add_epi64(base, _mm512_srai_epi64::<16>(p)), r0);
        p = _mm512_add_epi64(p, step);
        let t1 = _mm512_add_epi64(_mm512_add_epi64(base, _mm512_srai_epi64::<16>(p)), r1);
        p = _mm512_add_epi64(p, step);
        // SAFETY: b < BLOCKS, so i64s 16 b .. 16 b + 16 lie inside out; storeu has no alignment requirement.
        unsafe {
            _mm512_storeu_si512(o.add(16 * b).cast(), t0);
            _mm512_storeu_si512(o.add(16 * b + 8).cast(), t1);
        }
    }
}

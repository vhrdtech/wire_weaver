//! [int_batch] compiled for AVX2 and for AVX-512: the same code, vectorised for wider registers.
//!
//! A kernel is a token that exists only once its CPU features were detected, so a call into its `target_feature`
//! code is sound wherever the token is.

#![expect(
    unsafe_code,
    reason = "calls into target_feature functions, behind a token proving the CPU has the features"
)]

use super::{IntBatch, Narrow, int_batch};

/// Proof that this CPU has AVX2.
#[derive(Copy, Clone, Debug)]
pub(crate) struct Avx2(());

impl Avx2 {
    pub(crate) fn detect() -> Option<Self> {
        std::arch::is_x86_feature_detected!("avx2").then_some(Avx2(()))
    }

    #[inline]
    pub(crate) fn int_batch<T: Narrow, const C: usize>(
        self,
        frames: &[[T; C]],
        lo: T,
        hi: T,
        out: &mut IntBatch<T>,
    ) {
        // SAFETY: an Avx2 is made only by detect, after the CPU reported AVX2
        unsafe { avx2(frames, lo, hi, out) }
    }
}

/// Proof that this CPU has AVX-512 F, BW, DQ and VL.
#[derive(Copy, Clone, Debug)]
pub(crate) struct Avx512(());

impl Avx512 {
    pub(crate) fn detect() -> Option<Self> {
        (std::arch::is_x86_feature_detected!("avx512f")
            && std::arch::is_x86_feature_detected!("avx512bw")
            && std::arch::is_x86_feature_detected!("avx512dq")
            && std::arch::is_x86_feature_detected!("avx512vl"))
        .then_some(Avx512(()))
    }

    #[inline]
    pub(crate) fn int_batch<T: Narrow, const C: usize>(
        self,
        frames: &[[T; C]],
        lo: T,
        hi: T,
        out: &mut IntBatch<T>,
    ) {
        // SAFETY: an Avx512 is made only by detect, after the CPU reported every feature avx512 enables
        unsafe { avx512(frames, lo, hi, out) }
    }
}

#[target_feature(enable = "avx2")]
fn avx2<T: Narrow, const C: usize>(frames: &[[T; C]], lo: T, hi: T, out: &mut IntBatch<T>) {
    int_batch(frames, lo, hi, out);
}

#[target_feature(enable = "avx512f,avx512bw,avx512dq,avx512vl")]
fn avx512<T: Narrow, const C: usize>(frames: &[[T; C]], lo: T, hi: T, out: &mut IntBatch<T>) {
    int_batch(frames, lo, hi, out);
}

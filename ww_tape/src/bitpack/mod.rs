//! Bit packing: groups of up to [GROUP] unsigned values at one width, read in place.
//!
//! The layout is horizontal: value j of a group sits at bit `j × width`, LSB first, exactly `width` bits each (0 to
//! 64), so a group of n values takes [packed_len]`(n, width)` bytes. One value reads with one unaligned load
//! ([Width::read]); a whole group unpacks with a hand-written kernel ([Width::unpack_u32], [Width::decode_line]):
//! AVX-512 (VBMI and VBMI2 byte permutes and funnel shifts) or AVX2 (byte shuffles and variable shifts) on x86-64,
//! chosen at run time with `std` and at build time without it, NEON `tbl` on aarch64 targets built with NEON (not the
//! soft-float ones), and scalar code elsewhere. Every kernel gives the same values as the scalar code. Widths over 32
//! take the scalar path.
//!
//! A width read from a file becomes a [Width] once ([Width::from_file], [Width::new]), which checks it against
//! [WIDTH_MAX]; reads and unpacks then take it without a check per value. The free functions taking a `u32` width
//! panic on a width over the maximum instead; codecs move to [Width] (STD-131).
//!
//! Reads never go past the slice they are given. A slice that ends less than the kernel's reach after a group (the
//! last group of a column without [PADDING] bytes after it) is copied to the stack first, so in-place reads should
//! hand over the bytes after the group too: a chunk's payload ends with [PADDING] zero bytes for that.
//!
//! The event time codec and integer sample codecs share this module.

#[cfg(target_arch = "x86_64")]
mod avx2;
#[cfg(target_arch = "x86_64")]
mod avx512;
#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
mod neon;
#[cfg(test)]
mod tests;

use crate::from_file::FileInt;
use crate::{BoundError, FromFile, bytes};

/// Values in a full group.
pub const GROUP: usize = 128;
/// Widest value, in bits.
pub const WIDTH_MAX: u32 = 64;
/// Widest value the vector kernels unpack; wider groups take the scalar path.
pub const KERNEL_WIDTH_MAX: u32 = 32;
/// Bytes after the packed data that keep every read of [read] on its fast path: one 16-byte load from the byte holding
/// the last value's first bit.
pub const PADDING: usize = 16;

/// [WIDTH_MAX] as the byte a [Width] holds.
const WIDTH_MAX_BYTE: u8 = 64;
/// [KERNEL_WIDTH_MAX] as the byte a [Width] holds.
const KERNEL_WIDTH_MAX_BYTE: u8 = 32;

/// Bytes a group may reach from its start: the largest of the kernels' last loads (512 bytes at width 32) and of the
/// scalar 16-byte loads (1032 bytes at width 64).
const REACH_MAX: usize = scalar_reach(WIDTH_MAX_BYTE);

/// Why a group can't be packed or unpacked.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BitpackError {
    /// The width is over the largest the call takes: [WIDTH_MAX], or [KERNEL_WIDTH_MAX] for 32-bit values.
    Width(BoundError),
    /// The output buffer is shorter than the packed values.
    Full,
    /// The kernel doesn't run on this CPU.
    Kernel(Kernel),
}

impl core::fmt::Display for BitpackError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BitpackError::Width(e) => write!(f, "bit width: {e}"),
            BitpackError::Full => write!(f, "output buffer shorter than the packed values"),
            BitpackError::Kernel(k) => write!(f, "{k:?} doesn't run on this CPU"),
        }
    }
}

impl core::error::Error for BitpackError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        match self {
            BitpackError::Width(e) => Some(e),
            BitpackError::Full | BitpackError::Kernel(_) => None,
        }
    }
}

/// A bit width of packed values, 0 to [WIDTH_MAX]: checked once, so reads and unpacks take it without a check.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Width(u8);

impl Width {
    /// No bits: every value is 0.
    pub const ZERO: Width = Width(0);
    /// [WIDTH_MAX] bits.
    pub const MAX: Width = Width(WIDTH_MAX_BYTE);

    /// `bits` as a width, when it is at most [WIDTH_MAX].
    pub fn new(bits: u32) -> Result<Self, BoundError> {
        match u8::try_from(bits) {
            Ok(b) if b <= WIDTH_MAX_BYTE => Ok(Width(b)),
            _ => Err(BoundError {
                value: u64::from(bits),
                bound: u64::from(WIDTH_MAX),
            }),
        }
    }

    /// A width read from a file, when it is at most [WIDTH_MAX].
    pub fn from_file<T: FileInt>(bits: FromFile<T>) -> Result<Self, BoundError> {
        let b = bits.at_most(usize::from(WIDTH_MAX_BYTE))?;
        // b is at most 64: its low byte is all of it
        let [low, ..] = b.to_le_bytes();
        Ok(Width(low))
    }

    /// The bits needed to hold `x`: [width_of].
    pub const fn of(x: u64) -> Self {
        let [low, ..] = width_of(x).to_le_bytes();
        Width(low)
    }

    /// The width in bits.
    pub fn bits(self) -> u32 {
        u32::from(self.0)
    }

    /// Value `index` of values packed at this width from the start of `src`. Bits past the end of `src` read as 0.
    #[inline]
    pub fn read(self, src: &[u8], index: usize) -> u64 {
        // a bit past usize::MAX is past the end of every slice, and reads as 0 like one
        let bit = index.saturating_mul(usize::from(self.0));
        let (byte, sh) = (bit >> 3, bit & 7);
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "byte is at most usize::MAX / 8: 16 more can't overflow"
        )]
        let (end8, end16) = (byte + 8, byte + 16);
        let m = mask64(self.bits());
        if self.0 <= 56
            && let Some(Ok(b)) = src.get(byte..end8).map(<[u8; 8]>::try_from)
        {
            return (u64::from_le_bytes(b) >> sh) & m;
        }
        let b = match src.get(byte..end16).map(<[u8; 16]>::try_from) {
            Some(Ok(b)) => b,
            _ => tail16(src, byte),
        };
        low64(u128::from_le_bytes(b) >> sh) & m
    }

    /// Pack `vals` at this width into `out`, LSB first; returns the bytes written, [packed_len] of the count. Bits of
    /// a value above the width are dropped. The last byte's unused bits are 0; bytes after it are untouched.
    ///
    /// [BitpackError::Full] when `out` is shorter than the packed values; `out` is then partly written.
    pub fn pack(
        self,
        out: &mut [u8],
        vals: impl IntoIterator<Item = u64>,
    ) -> Result<usize, BitpackError> {
        if self.0 == 0 {
            return Ok(0);
        }
        let w = usize::from(self.0);
        let m = mask64(self.bits());
        let mut acc: u128 = 0;
        // bits held in acc: under 64 between values
        let mut n: usize = 0;
        let mut at = 0;
        for v in vals {
            #[expect(
                clippy::arithmetic_side_effects,
                reason = "n < 64 between values and w <= 64: n + w < 128, the bits of acc"
            )]
            {
                acc |= u128::from(v & m) << n;
                n += w;
            }
            if n >= 64 {
                bytes::put(out, at, low64(acc).to_le_bytes()).ok_or(BitpackError::Full)?;
                #[expect(
                    clippy::arithmetic_side_effects,
                    reason = "put wrote 8 bytes at `at`, so at + 8 <= out.len(); n >= 64 here"
                )]
                {
                    at += 8;
                    n -= 64;
                }
                acc >>= 64;
            }
        }
        let tail = n.div_ceil(8);
        let last = low64(acc).to_le_bytes();
        let (Some(dst), Some(src)) = (
            out.get_mut(at..).and_then(|o| o.get_mut(..tail)),
            last.get(..tail),
        ) else {
            return Err(BitpackError::Full);
        };
        dst.copy_from_slice(src);
        #[expect(
            clippy::arithmetic_side_effects,
            reason = "the tail's bytes were written at `at`: at + tail <= out.len()"
        )]
        let len = at + tail;
        Ok(len)
    }

    /// Unpack a group of values of at most [KERNEL_WIDTH_MAX] bits from the start of `src` with the fastest kernel.
    /// Values past the group's real count are whatever bits follow it; bits past the end of `src` read as 0.
    ///
    /// [BitpackError::Width] when the width is over [KERNEL_WIDTH_MAX].
    #[inline]
    pub fn unpack_u32(self, src: &[u8], out: &mut [u32; GROUP]) -> Result<(), BitpackError> {
        self.unpack_u32_with(Kernel::best(), src, out)
    }

    /// [Width::unpack_u32] with a given kernel; [BitpackError::Kernel] when this CPU doesn't run it.
    pub fn unpack_u32_with(
        self,
        kernel: Kernel,
        src: &[u8],
        out: &mut [u32; GROUP],
    ) -> Result<(), BitpackError> {
        if self.0 > KERNEL_WIDTH_MAX_BYTE {
            return Err(BitpackError::Width(BoundError {
                value: u64::from(self.0),
                bound: u64::from(KERNEL_WIDTH_MAX),
            }));
        }
        if !kernel.is_available() {
            return Err(BitpackError::Kernel(kernel));
        }
        unpack32(kernel, src, self.0, out);
        Ok(())
    }

    /// Unpack a group of values of this width from the start of `src`; values past the group's real count and bits
    /// past the end of `src` as in [Width::unpack_u32].
    pub fn unpack_u64(self, src: &[u8], out: &mut [u64; GROUP]) {
        if self.0 <= KERNEL_WIDTH_MAX_BYTE {
            let mut r = [0u32; GROUP];
            unpack32(Kernel::best(), src, self.0, &mut r);
            for (o, x) in out.iter_mut().zip(r) {
                *o = u64::from(x);
            }
            return;
        }
        let w = self.0;
        with_reach(src, scalar_reach(w), |src| unpack_scalar64(src, w, out));
    }

    /// Decode a group to `base + line(slope, j) + value[j]` (wrapping) with the fastest kernel; values as in
    /// [Width::unpack_u64].
    #[inline]
    pub fn decode_line(self, src: &[u8], base: i64, slope: i64, out: &mut [i64; GROUP]) {
        decode(Kernel::best(), src, self.0, base, slope, out);
    }

    /// [Width::decode_line] with a given kernel (used for widths up to [KERNEL_WIDTH_MAX]); [BitpackError::Kernel]
    /// when this CPU doesn't run it.
    pub fn decode_line_with(
        self,
        kernel: Kernel,
        src: &[u8],
        base: i64,
        slope: i64,
        out: &mut [i64; GROUP],
    ) -> Result<(), BitpackError> {
        if !kernel.is_available() {
            return Err(BitpackError::Kernel(kernel));
        }
        decode(kernel, src, self.0, base, slope, out);
        Ok(())
    }
}

/// `w` as a [Width].
///
/// # Panics
///
/// When `w` is over [WIDTH_MAX].
#[track_caller]
#[inline(always)]
fn checked(w: u32) -> Width {
    let r = Width::new(w);
    assert!(r.is_ok(), "width {w} over {WIDTH_MAX}");
    r.unwrap_or(Width::ZERO)
}

/// Bits needed to hold `x`: 0 for 0, 64 for values from 2^63.
pub const fn width_of(x: u64) -> u32 {
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "leading_zeros is at most u64::BITS"
    )]
    let w = u64::BITS - x.leading_zeros();
    w
}

/// Bytes of `n` values packed at `width` bits; usize::MAX, a length no slice holds, when it is more.
pub const fn packed_len(n: usize, width: u32) -> usize {
    const { assert!(usize::BITS >= u32::BITS, "a u32 fits a usize") };
    #[expect(
        clippy::as_conversions,
        reason = "a u32 fits a usize (asserted above); From is not const"
    )]
    let w = width as usize;
    // n = 8 q + r: q values take `width` whole bytes
    let (q, r) = (n / 8, n % 8);
    q.saturating_mul(w)
        .saturating_add(r.saturating_mul(w).div_ceil(8))
}

/// The low `width` bits set: all of them from 64.
#[inline(always)]
const fn mask64(width: u32) -> u64 {
    // a shift by 64 is None: no bits
    match u64::MAX.checked_shr(u64::BITS.saturating_sub(width)) {
        Some(m) => m,
        None => 0,
    }
}

/// The low `width` bits set: all of them from 32.
#[inline(always)]
const fn mask32(width: u32) -> u32 {
    match u32::MAX.checked_shr(u32::BITS.saturating_sub(width)) {
        Some(m) => m,
        None => 0,
    }
}

/// The low 64 bits of `x`.
#[inline(always)]
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "dropping the high bits is the meaning"
)]
const fn low64(x: u128) -> u64 {
    x as u64
}

/// The low 32 bits of `x`.
#[inline(always)]
#[expect(
    clippy::as_conversions,
    clippy::cast_possible_truncation,
    reason = "dropping the high bits is the meaning"
)]
const fn low32(x: u64) -> u32 {
    x as u32
}

/// The 16 bytes at `at` of `src` with zeros past its end.
#[cold]
#[inline(never)]
fn tail16(src: &[u8], at: usize) -> [u8; 16] {
    let mut b = [0u8; 16];
    for (d, s) in b.iter_mut().zip(src.get(at..).unwrap_or_default()) {
        *d = *s;
    }
    b
}

/// Value `index` of values packed at `width` bits from the start of `src`: [Width::read].
///
/// # Panics
///
/// When `width` is over 64.
#[inline]
pub fn read(src: &[u8], index: usize, width: u32) -> u64 {
    checked(width).read(src, index)
}

/// Pack `vals` at `width` bits each into `out`: [Width::pack].
///
/// # Panics
///
/// When `width` is over 64 or `out` is shorter than the packed values.
pub fn pack(out: &mut [u8], vals: impl IntoIterator<Item = u64>, width: u32) -> usize {
    let r = checked(width).pack(out, vals);
    assert!(r.is_ok(), "out shorter than the packed values");
    r.unwrap_or(0)
}

/// A group unpacker.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Kernel {
    /// Plain Rust, every target.
    Scalar,
    /// x86-64 AVX2.
    Avx2,
    /// x86-64 AVX-512 F, BW, VBMI and VBMI2.
    Avx512,
    /// aarch64 NEON, on targets built with it.
    Neon,
}

impl Kernel {
    /// The fastest kernel this CPU runs.
    pub fn best() -> Self {
        [Kernel::Avx512, Kernel::Avx2, Kernel::Neon]
            .into_iter()
            .find(|k| k.is_available())
            .unwrap_or(Kernel::Scalar)
    }

    /// Whether this CPU runs the kernel.
    pub fn is_available(self) -> bool {
        match self {
            Kernel::Scalar => true,
            Kernel::Avx2 => has_avx2(),
            Kernel::Avx512 => has_avx512(),
            Kernel::Neon => cfg!(all(target_arch = "aarch64", target_feature = "neon")),
        }
    }
}

#[cfg(all(target_arch = "x86_64", feature = "std"))]
fn has_avx2() -> bool {
    std::arch::is_x86_feature_detected!("avx2")
}

#[cfg(all(target_arch = "x86_64", not(feature = "std")))]
fn has_avx2() -> bool {
    cfg!(target_feature = "avx2")
}

#[cfg(not(target_arch = "x86_64"))]
fn has_avx2() -> bool {
    false
}

#[cfg(all(target_arch = "x86_64", feature = "std"))]
fn has_avx512() -> bool {
    std::arch::is_x86_feature_detected!("avx512f")
        && std::arch::is_x86_feature_detected!("avx512bw")
        && std::arch::is_x86_feature_detected!("avx512vbmi")
        && std::arch::is_x86_feature_detected!("avx512vbmi2")
}

#[cfg(all(target_arch = "x86_64", not(feature = "std")))]
fn has_avx512() -> bool {
    cfg!(all(
        target_feature = "avx512f",
        target_feature = "avx512bw",
        target_feature = "avx512vbmi",
        target_feature = "avx512vbmi2"
    ))
}

#[cfg(not(target_arch = "x86_64"))]
fn has_avx512() -> bool {
    false
}

/// Call `f` on `src` when it holds `reach` bytes, else on its first bytes copied to the stack with zeros after them.
/// `reach` is at most [REACH_MAX].
#[inline(always)]
fn with_reach<R>(src: &[u8], reach: usize, f: impl FnOnce(&[u8]) -> R) -> R {
    if src.len() >= reach {
        f(src)
    } else {
        short(src, reach, f)
    }
}

#[cold]
#[inline(never)]
fn short<R>(src: &[u8], reach: usize, f: impl FnOnce(&[u8]) -> R) -> R {
    debug_assert!(reach <= REACH_MAX, "reach {reach} over {REACH_MAX}");
    let mut buf = [0u8; REACH_MAX];
    for (d, s) in buf.iter_mut().zip(src) {
        *d = *s;
    }
    // a reach over REACH_MAX hands over all of buf, and the kernel's length check refuses it
    f(buf.get(..reach).unwrap_or(&buf))
}

/// Bytes the scalar unpack of a full group reaches: a 16-byte load at the last value's first byte.
const fn scalar_reach(width: u8) -> usize {
    #[expect(
        clippy::as_conversions,
        reason = "u8 to usize is lossless; From is not const"
    )]
    let w = width as usize;
    #[expect(
        clippy::arithmetic_side_effects,
        reason = "w is at most 255: at most 127 × 255 / 8 + 16"
    )]
    let reach = (GROUP - 1) * w / 8 + 16;
    reach
}

/// Unpack a group at `width` (at most 32) with `kernel`, which this CPU runs.
fn unpack32(kernel: Kernel, src: &[u8], width: u8, out: &mut [u32; GROUP]) {
    if width == 0 {
        out.fill(0);
        return;
    }
    match kernel {
        #[cfg(target_arch = "x86_64")]
        Kernel::Avx2 => with_reach(src, avx2::reach(width), |s| avx2::unpack(s, width, out)),
        #[cfg(target_arch = "x86_64")]
        Kernel::Avx512 => with_reach(src, avx512::reach(width), |s| avx512::unpack(s, width, out)),
        #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
        Kernel::Neon => with_reach(src, neon::reach(width), |s| neon::unpack(s, width, out)),
        _ => with_reach(src, scalar_reach(width), |s| unpack_scalar(s, width, out)),
    }
}

/// Unpack a group at `w` (1 to 32) from `src`, which holds [scalar_reach]`(w)` bytes ([with_reach]).
#[expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "j < GROUP and w <= 32: j w / 8 + 8 <= scalar_reach(w), the bytes with_reach hands over"
)]
fn unpack_scalar(src: &[u8], w: u8, out: &mut [u32; GROUP]) {
    debug_assert!(src.len() >= scalar_reach(w), "src shorter than the reach");
    let m = u64::from(mask32(u32::from(w)));
    let w = usize::from(w);
    for (j, o) in out.iter_mut().enumerate() {
        let bit = j * w;
        let b = bit >> 3;
        let mut x = [0u8; 8];
        x.copy_from_slice(&src[b..b + 8]);
        let x = u64::from_le_bytes(x);
        *o = low32((x >> (bit & 7)) & m);
    }
}

/// Unpack a group at `w` (33 to 64) from `src`, which holds [scalar_reach]`(w)` bytes ([with_reach]).
#[expect(
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "j < GROUP and w <= 64: j w / 8 + 16 <= scalar_reach(w), the bytes with_reach hands over"
)]
fn unpack_scalar64(src: &[u8], w: u8, out: &mut [u64; GROUP]) {
    debug_assert!(src.len() >= scalar_reach(w), "src shorter than the reach");
    let m = mask64(u32::from(w));
    let w = usize::from(w);
    for (j, o) in out.iter_mut().enumerate() {
        let bit = j * w;
        let b = bit >> 3;
        let mut x = [0u8; 16];
        x.copy_from_slice(&src[b..b + 16]);
        let x = u128::from_le_bytes(x);
        *o = low64(x >> (bit & 7)) & m;
    }
}

/// Decode a group at `width` (at most 64) with `kernel`, which this CPU runs.
fn decode(kernel: Kernel, src: &[u8], width: u8, base: i64, slope: i64, out: &mut [i64; GROUP]) {
    if width > KERNEL_WIDTH_MAX_BYTE {
        let mut r = [0u64; GROUP];
        with_reach(src, scalar_reach(width), |s| {
            unpack_scalar64(s, width, &mut r)
        });
        let mut p = 0i64;
        for (o, x) in out.iter_mut().zip(r) {
            *o = base.wrapping_add(p >> 16).wrapping_add(x.cast_signed());
            p = p.wrapping_add(slope);
        }
        return;
    }
    match kernel {
        #[cfg(target_arch = "x86_64")]
        Kernel::Avx2 if width > 0 => with_reach(src, avx2::reach(width), |s| {
            avx2::decode(s, width, base, slope, out)
        }),
        #[cfg(target_arch = "x86_64")]
        Kernel::Avx512 if width > 0 => with_reach(src, avx512::reach(width), |s| {
            avx512::decode(s, width, base, slope, out)
        }),
        #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
        Kernel::Neon if width > 0 => with_reach(src, neon::reach(width), |s| {
            neon::decode(s, width, base, slope, out)
        }),
        _ => {
            let mut r = [0u32; GROUP];
            if width > 0 {
                with_reach(src, scalar_reach(width), |s| {
                    unpack_scalar(s, width, &mut r)
                });
            }
            // a running sum of the slope: no multiply per value
            let mut p = 0i64;
            for (o, x) in out.iter_mut().zip(r) {
                *o = base.wrapping_add(p >> 16).wrapping_add(i64::from(x));
                p = p.wrapping_add(slope);
            }
        }
    }
}

/// Unpack a group of values of at most 32 bits from the start of `src` with the fastest kernel: [Width::unpack_u32].
///
/// # Panics
///
/// When `width` is over [KERNEL_WIDTH_MAX].
#[inline]
pub fn unpack_u32(src: &[u8], width: u32, out: &mut [u32; GROUP]) {
    unpack_u32_with(Kernel::best(), src, width, out);
}

/// [unpack_u32] with a given kernel: [Width::unpack_u32_with].
///
/// # Panics
///
/// When `width` is over [KERNEL_WIDTH_MAX] or this CPU doesn't run `kernel`.
pub fn unpack_u32_with(kernel: Kernel, src: &[u8], width: u32, out: &mut [u32; GROUP]) {
    assert!(
        width <= KERNEL_WIDTH_MAX,
        "width {width} over {KERNEL_WIDTH_MAX}"
    );
    let r = checked(width).unpack_u32_with(kernel, src, out);
    assert!(r.is_ok(), "{kernel:?} doesn't run on this CPU");
}

/// Unpack a group of values of any width up to 64 from the start of `src`: [Width::unpack_u64].
///
/// # Panics
///
/// When `width` is over 64.
pub fn unpack_u64(src: &[u8], width: u32, out: &mut [u64; GROUP]) {
    checked(width).unpack_u64(src, out);
}

/// The line of value j: `(j × slope) >> 16`, wrapping, with an arithmetic shift (slope in Q16).
#[inline(always)]
pub const fn line(slope: i64, j: usize) -> i64 {
    #[expect(
        clippy::as_conversions,
        clippy::cast_possible_wrap,
        reason = "the line wraps: an index past i64::MAX wraps with it"
    )]
    let j = j as i64;
    j.wrapping_mul(slope) >> 16
}

/// Decode a group with the fastest kernel: [Width::decode_line].
///
/// # Panics
///
/// When `width` is over 64.
#[inline]
pub fn decode_line(src: &[u8], width: u32, base: i64, slope: i64, out: &mut [i64; GROUP]) {
    checked(width).decode_line(src, base, slope, out);
}

/// [decode_line] with a given kernel: [Width::decode_line_with].
///
/// # Panics
///
/// When `width` is over 64 or this CPU doesn't run `kernel`.
pub fn decode_line_with(
    kernel: Kernel,
    src: &[u8],
    width: u32,
    base: i64,
    slope: i64,
    out: &mut [i64; GROUP],
) {
    let r = checked(width).decode_line_with(kernel, src, base, slope, out);
    assert!(r.is_ok(), "{kernel:?} doesn't run on this CPU");
}

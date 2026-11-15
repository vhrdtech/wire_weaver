//! Bit packing: groups of up to [GROUP] unsigned values at one width, read in place.
//!
//! The layout is horizontal: value j of a group sits at bit `j × width`, LSB first, exactly `width` bits each (0 to
//! 64), so a group of n values takes [packed_len]`(n, width)` bytes. One value reads with one unaligned load
//! ([read]); a whole group unpacks with a hand-written kernel ([unpack_u32], [decode_line]): AVX-512 (VBMI and VBMI2
//! byte permutes and funnel shifts) or AVX2 (byte shuffles and variable shifts) on x86-64, chosen at run time with
//! `std` and at build time without it, NEON `tbl` on aarch64, and scalar code elsewhere. Every kernel gives the same
//! values as the scalar code. Widths over 32 take the scalar path.
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
#[cfg(target_arch = "aarch64")]
mod neon;
#[cfg(test)]
mod tests;

/// Values in a full group.
pub const GROUP: usize = 128;
/// Widest value, in bits.
pub const WIDTH_MAX: u32 = 64;
/// Widest value the vector kernels unpack; wider groups take the scalar path.
pub const KERNEL_WIDTH_MAX: u32 = 32;
/// Bytes after the packed data that keep every read of [read] on its fast path: one 16-byte load from the byte holding
/// the last value's first bit.
pub const PADDING: usize = 16;

/// Bytes a group may reach from its start: the largest of the kernels' last loads (512 bytes at width 32) and of the
/// scalar 16-byte loads (1032 bytes at width 64).
const REACH_MAX: usize = (GROUP - 1) * WIDTH_MAX as usize / 8 + 16;

/// Bits needed to hold `x`: 0 for 0, 64 for values from 2^63.
pub const fn width_of(x: u64) -> u32 {
    64 - x.leading_zeros()
}

/// Bytes of `n` values packed at `width` bits.
pub const fn packed_len(n: usize, width: u32) -> usize {
    (n * width as usize).div_ceil(8)
}

/// The low `width` bits set.
#[inline(always)]
const fn mask64(width: u32) -> u64 {
    if width >= 64 {
        u64::MAX
    } else {
        (1u64 << width) - 1
    }
}

#[inline(always)]
const fn mask32(width: u32) -> u32 {
    if width >= 32 {
        u32::MAX
    } else {
        (1u32 << width) - 1
    }
}

/// Pack `vals` at `width` bits each into `out`, LSB first; returns the bytes written, [packed_len] of the count.
/// Bits of a value above `width` are dropped. The last byte's unused bits are 0; bytes after it are untouched.
///
/// # Panics
///
/// When `width` is over 64 or `out` is shorter than the packed values.
pub fn pack(out: &mut [u8], vals: impl IntoIterator<Item = u64>, width: u32) -> usize {
    assert!(width <= WIDTH_MAX, "width {width} over {WIDTH_MAX}");
    if width == 0 {
        return 0;
    }
    let m = mask64(width);
    let mut acc: u128 = 0;
    let mut n: u32 = 0;
    let mut at = 0;
    for v in vals {
        acc |= u128::from(v & m) << n;
        n += width;
        if n >= 64 {
            out[at..at + 8].copy_from_slice(&(acc as u64).to_le_bytes());
            at += 8;
            acc >>= 64;
            n -= 64;
        }
    }
    let tail = n.div_ceil(8) as usize;
    out[at..at + tail].copy_from_slice(&(acc as u64).to_le_bytes()[..tail]);
    at + tail
}

/// Value `index` of values packed at `width` bits from the start of `src`. Bits past the end of `src` read as 0.
///
/// # Panics
///
/// When `width` is over 64.
#[inline]
pub fn read(src: &[u8], index: usize, width: u32) -> u64 {
    assert!(width <= WIDTH_MAX, "width {width} over {WIDTH_MAX}");
    let bit = index * width as usize;
    let (byte, sh) = (bit >> 3, (bit & 7) as u32);
    if let Some(b) = src.get(byte..byte + 16) {
        let w = u128::from_le_bytes(b.try_into().expect("16 bytes"));
        ((w >> sh) as u64) & mask64(width)
    } else {
        let mut b = [0u8; 16];
        let have = src.len().saturating_sub(byte).min(16);
        if have > 0 {
            b[..have].copy_from_slice(&src[byte..byte + have]);
        }
        ((u128::from_le_bytes(b) >> sh) as u64) & mask64(width)
    }
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
    /// aarch64 NEON.
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
            Kernel::Neon => cfg!(target_arch = "aarch64"),
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

/// `src` when it holds `reach` bytes, else its first bytes copied into `buf` with zeros after them.
fn reach<'a>(src: &'a [u8], reach: usize, buf: &'a mut [u8; REACH_MAX]) -> &'a [u8] {
    debug_assert!(reach <= REACH_MAX, "reach {reach} over {REACH_MAX}");
    if src.len() >= reach {
        return src;
    }
    buf[..src.len()].copy_from_slice(src);
    buf[src.len()..reach].fill(0);
    &buf[..reach]
}

/// Bytes the scalar unpack of a full group reaches: a 16-byte load at the last value's first byte.
const fn scalar_reach(width: u32) -> usize {
    (GROUP - 1) * width as usize / 8 + 16
}

/// Unpack a group of values of at most 32 bits from the start of `src` with the fastest kernel. Values past the
/// group's real count are whatever bits follow it; bits past the end of `src` read as 0.
///
/// # Panics
///
/// When `width` is over [KERNEL_WIDTH_MAX].
#[inline]
pub fn unpack_u32(src: &[u8], width: u32, out: &mut [u32; GROUP]) {
    unpack_u32_with(Kernel::best(), src, width, out);
}

/// [unpack_u32] with a given kernel.
///
/// # Panics
///
/// When `width` is over [KERNEL_WIDTH_MAX] or this CPU doesn't run `kernel`.
pub fn unpack_u32_with(kernel: Kernel, src: &[u8], width: u32, out: &mut [u32; GROUP]) {
    assert!(
        width <= KERNEL_WIDTH_MAX,
        "width {width} over {KERNEL_WIDTH_MAX}"
    );
    assert!(kernel.is_available(), "{kernel:?} doesn't run on this CPU");
    if width == 0 {
        out.fill(0);
        return;
    }
    let mut buf = [0u8; REACH_MAX];
    match kernel {
        #[cfg(target_arch = "x86_64")]
        Kernel::Avx2 => avx2::unpack(reach(src, avx2::reach(width), &mut buf), width, out),
        #[cfg(target_arch = "x86_64")]
        Kernel::Avx512 => avx512::unpack(reach(src, avx512::reach(width), &mut buf), width, out),
        #[cfg(target_arch = "aarch64")]
        Kernel::Neon => neon::unpack(reach(src, neon::reach(width), &mut buf), width, out),
        _ => unpack_scalar(reach(src, scalar_reach(width), &mut buf), width, out),
    }
}

fn unpack_scalar(src: &[u8], width: u32, out: &mut [u32; GROUP]) {
    let m = u64::from(mask32(width));
    for (j, o) in out.iter_mut().enumerate() {
        let bit = j * width as usize;
        let b = bit >> 3;
        let w = u64::from_le_bytes(src[b..b + 8].try_into().expect("8 bytes"));
        *o = ((w >> (bit & 7)) & m) as u32;
    }
}

/// Unpack a group of values of any width up to 64 from the start of `src`; values past the group's real count and
/// bits past the end of `src` as in [unpack_u32].
///
/// # Panics
///
/// When `width` is over 64.
pub fn unpack_u64(src: &[u8], width: u32, out: &mut [u64; GROUP]) {
    assert!(width <= WIDTH_MAX, "width {width} over {WIDTH_MAX}");
    if width <= KERNEL_WIDTH_MAX {
        let mut r = [0u32; GROUP];
        unpack_u32(src, width, &mut r);
        for (o, x) in out.iter_mut().zip(r) {
            *o = u64::from(x);
        }
        return;
    }
    let mut buf = [0u8; REACH_MAX];
    let src = reach(src, scalar_reach(width), &mut buf);
    let m = mask64(width);
    for (j, o) in out.iter_mut().enumerate() {
        let bit = j * width as usize;
        let b = bit >> 3;
        let w = u128::from_le_bytes(src[b..b + 16].try_into().expect("16 bytes"));
        *o = ((w >> (bit & 7)) as u64) & m;
    }
}

/// The line of value j: `(j × slope) >> 16`, wrapping, with an arithmetic shift (slope in Q16).
#[inline(always)]
pub const fn line(slope: i64, j: usize) -> i64 {
    (j as i64).wrapping_mul(slope) >> 16
}

/// Decode a group to `base + line(slope, j) + value[j]` (wrapping) with the fastest kernel; values as in
/// [unpack_u64].
///
/// # Panics
///
/// When `width` is over 64.
#[inline]
pub fn decode_line(src: &[u8], width: u32, base: i64, slope: i64, out: &mut [i64; GROUP]) {
    decode_line_with(Kernel::best(), src, width, base, slope, out);
}

/// [decode_line] with a given kernel (used for widths up to [KERNEL_WIDTH_MAX]).
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
    assert!(width <= WIDTH_MAX, "width {width} over {WIDTH_MAX}");
    assert!(kernel.is_available(), "{kernel:?} doesn't run on this CPU");
    if width > KERNEL_WIDTH_MAX {
        let mut r = [0u64; GROUP];
        unpack_u64(src, width, &mut r);
        let mut p = 0i64;
        for (o, x) in out.iter_mut().zip(r) {
            *o = base.wrapping_add(p >> 16).wrapping_add(x as i64);
            p = p.wrapping_add(slope);
        }
        return;
    }
    let mut buf = [0u8; REACH_MAX];
    match kernel {
        #[cfg(target_arch = "x86_64")]
        Kernel::Avx2 if width > 0 => avx2::decode(
            reach(src, avx2::reach(width), &mut buf),
            width,
            base,
            slope,
            out,
        ),
        #[cfg(target_arch = "x86_64")]
        Kernel::Avx512 if width > 0 => avx512::decode(
            reach(src, avx512::reach(width), &mut buf),
            width,
            base,
            slope,
            out,
        ),
        #[cfg(target_arch = "aarch64")]
        Kernel::Neon if width > 0 => neon::decode(
            reach(src, neon::reach(width), &mut buf),
            width,
            base,
            slope,
            out,
        ),
        _ => {
            let mut r = [0u32; GROUP];
            if width > 0 {
                unpack_scalar(reach(src, scalar_reach(width), &mut buf), width, &mut r);
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

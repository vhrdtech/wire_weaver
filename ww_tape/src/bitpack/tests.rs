use super::*;

fn width(w: u32) -> Width {
    Width::new(w).unwrap()
}

fn read(src: &[u8], index: usize, w: u32) -> u64 {
    width(w).read(src, index)
}

fn pack(out: &mut [u8], vals: impl IntoIterator<Item = u64>, w: u32) -> usize {
    width(w).pack(out, vals).unwrap()
}

fn unpack_u32(src: &[u8], w: u32, out: &mut [u32; GROUP]) {
    width(w).unpack_u32(src, out).unwrap();
}

fn unpack_u32_with(kernel: Kernel, src: &[u8], w: u32, out: &mut [u32; GROUP]) {
    width(w).unpack_u32_with(kernel, src, out).unwrap();
}

fn unpack_u64(src: &[u8], w: u32, out: &mut [u64; GROUP]) {
    width(w).unpack_u64(src, out);
}

fn decode_line_with(
    kernel: Kernel,
    src: &[u8],
    w: u32,
    base: i64,
    slope: i64,
    out: &mut [i64; GROUP],
) {
    width(w)
        .decode_line_with(kernel, src, base, slope, out)
        .unwrap();
}

/// xorshift64*, fixed seeds.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
}

const KERNELS: [Kernel; 4] = [Kernel::Scalar, Kernel::Avx2, Kernel::Avx512, Kernel::Neon];

/// Random values, all ones, a single high bit every third value.
fn cases(r: &mut Rng, w: u32) -> [[u64; GROUP]; 3] {
    let m = mask64(w);
    let high = if w == 0 { 0 } else { 1u64 << (w - 1) };
    [
        core::array::from_fn(|_| r.next() & m),
        [m; GROUP],
        core::array::from_fn(|j| if j % 3 == 0 { high } else { 0 }),
    ]
}

fn packed(vals: &[u64], w: u32, after: u8) -> Vec<u8> {
    let mut b = vec![after; packed_len(vals.len(), w) + 64];
    let n = pack(&mut b, vals.iter().copied(), w);
    assert_eq!(n, packed_len(vals.len(), w));
    b.truncate(n);
    b
}

#[test]
fn pack_and_read_every_width() {
    let mut r = Rng(1);
    for w in 0..=WIDTH_MAX {
        for vals in cases(&mut r, w) {
            for n in [1, 7, 100, GROUP] {
                let b = packed(&vals[..n], w, 0);
                for (j, v) in vals[..n].iter().enumerate() {
                    assert_eq!(read(&b, j, w), *v, "w {w} n {n} j {j}");
                }
            }
        }
    }
}

#[test]
fn pack_leaves_the_bytes_after_it() {
    let mut b = [0xaau8; 8];
    assert_eq!(pack(&mut b, [1, 0, 1], 3), 2);
    assert_eq!(b, [0b0100_0001, 0, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa, 0xaa]);
}

#[test]
fn every_kernel_equals_the_scalar_code() {
    let mut r = Rng(2);
    let lines = [
        (1_000_000_000, 65_535_938),
        (-5_000_000_000, -1_234_567),
        (0, 0),
        (7, 1 << 40),
        (i64::MAX - 3, i64::MAX / 200),
        (i64::MIN, 1 << 55),
    ];
    for w in 0..=WIDTH_MAX {
        for vals in cases(&mut r, w) {
            // back to back with a group of all ones after it, as in a column, and alone at the end of its bytes
            let mut both = vals.to_vec();
            both.extend([mask64(w); GROUP]);
            let follow = packed(&both, w, 0);
            let alone = packed(&vals, w, 0xff);
            for k in KERNELS.into_iter().filter(|k| k.is_available()) {
                for src in [&follow[..], &alone[..]] {
                    if w <= KERNEL_WIDTH_MAX {
                        let mut o = [0u32; GROUP];
                        unpack_u32_with(k, src, w, &mut o);
                        assert!(
                            o.iter().zip(&vals).all(|(a, b)| u64::from(*a) == *b),
                            "{k:?} unpack w {w}"
                        );
                    }
                    for (base, slope) in lines {
                        let want: [i64; GROUP] = core::array::from_fn(|j| {
                            base.wrapping_add(line(slope, j))
                                .wrapping_add(vals[j] as i64)
                        });
                        let mut o = [0i64; GROUP];
                        decode_line_with(k, src, w, base, slope, &mut o);
                        assert_eq!(o, want, "{k:?} decode w {w} line {base} {slope}");
                    }
                }
            }
            let mut o = [0u64; GROUP];
            unpack_u64(&alone, w, &mut o);
            assert_eq!(o, vals, "unpack_u64 w {w}");
        }
    }
}

#[test]
fn bits_past_the_slice_read_as_zero() {
    let b = packed(&[u64::MAX; 3], 64, 0);
    assert_eq!(read(&b, 3, 64), 0);
    assert_eq!(read(&b[..20], 2, 64), 0xffff_ffff);
    let mut o = [1u32; GROUP];
    unpack_u32(&b[..5], 8, &mut o);
    assert_eq!(&o[..6], &[0xff, 0xff, 0xff, 0xff, 0xff, 0]);
}

#[test]
fn the_best_kernel_runs_here() {
    assert!(Kernel::best().is_available());
    // NEON only where the build has it: aarch64 soft-float targets lack it
    let neon = cfg!(all(target_arch = "aarch64", target_feature = "neon"));
    assert_eq!(Kernel::Neon.is_available(), neon);
    if neon {
        assert_eq!(Kernel::best(), Kernel::Neon);
    }
}

#[test]
fn widths_over_the_maximum_are_refused() {
    assert_eq!(Width::new(64), Ok(Width::MAX));
    assert_eq!(Width::MAX.bits(), WIDTH_MAX);
    assert_eq!(
        Width::new(65),
        Err(BoundError {
            value: 65,
            bound: 64
        })
    );
    assert!(Width::new(u32::MAX).is_err());
    assert_eq!(Width::from_file(FromFile::new(64u8)), Ok(Width::MAX));
    assert!(Width::from_file(FromFile::new(65u8)).is_err());
    assert_eq!(Width::of(u64::MAX), Width::MAX);
    assert_eq!(Width::of(0), Width::ZERO);
    let src = [0xffu8; 600];
    let mut o = [0u32; GROUP];
    assert_eq!(
        Width::new(33).unwrap().unpack_u32(&src, &mut o),
        Err(BitpackError::Width(BoundError {
            value: 33,
            bound: 32
        }))
    );
    if !Kernel::Neon.is_available() {
        let w = Width::new(8).unwrap();
        assert_eq!(
            w.unpack_u32_with(Kernel::Neon, &src, &mut o),
            Err(BitpackError::Kernel(Kernel::Neon))
        );
        let mut d = [0i64; GROUP];
        assert_eq!(
            w.decode_line_with(Kernel::Neon, &src, 0, 0, &mut d),
            Err(BitpackError::Kernel(Kernel::Neon))
        );
    }
}

#[test]
fn a_short_output_is_an_error() {
    let w = Width::new(12).unwrap();
    // 3 values need 5 bytes: the tail doesn't fit
    assert_eq!(w.pack(&mut [0u8; 4], [1, 2, 3]), Err(BitpackError::Full));
    // 6 values need 9 bytes: a whole 8-byte word doesn't fit
    assert_eq!(w.pack(&mut [0u8; 8], [1; 6]), Err(BitpackError::Full));
    assert_eq!(w.pack(&mut [0u8; 9], [1; 6]), Ok(9));
}

#[test]
fn huge_indices_and_counts_do_not_overflow() {
    // index × width past usize::MAX overflowed: such a bit is past the end and reads as 0
    let b = [0xffu8; 32];
    assert_eq!(read(&b, usize::MAX, 64), 0);
    assert_eq!(read(&b, usize::MAX / 8 + 1, 9), 0);
    // n × width past usize::MAX overflowed: the length is more than any slice
    assert_eq!(packed_len(usize::MAX, 64), usize::MAX);
    assert_eq!(packed_len(usize::MAX, 1), usize::MAX.div_ceil(8));
    assert_eq!(packed_len(9, 7), 8);
    assert_eq!(packed_len(GROUP, 64), 1024);
}

#[test]
fn every_reach_fits_the_stack_copy() {
    for w in 1..=KERNEL_WIDTH_MAX_BYTE {
        #[cfg(target_arch = "x86_64")]
        {
            assert!(avx2::reach(w) <= REACH_MAX, "avx2 w {w}");
            assert!(avx512::reach(w) <= REACH_MAX, "avx512 w {w}");
        }
        #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
        assert!(neon::reach(w) <= REACH_MAX, "neon w {w}");
    }
    assert_eq!(scalar_reach(WIDTH_MAX_BYTE), REACH_MAX);
}

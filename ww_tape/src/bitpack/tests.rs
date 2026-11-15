use super::*;

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

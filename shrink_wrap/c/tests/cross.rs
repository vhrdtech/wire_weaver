//! Property-based cross-check of the C runtime against the Rust crate (SW-43), in both directions:
//! - C writes the same bytes as Rust, or fails with the same first error when the buffer is too small;
//! - C reads what Rust (and C) wrote back to the same values, and Rust reads what C wrote;
//! - on truncated or corrupted input, C reads the same values as Rust up to the first error, keeps that error,
//!   gives 0 for every read after it and stays where the failing read started (SW-41).

use proptest::prelude::*;
use shrink_wrap::BufReader;
use shrink_wrap_c::CReader;
use shrink_wrap_c::model::{Op, Val, check_reads, check_writes, expected, read_count};

fn leaf() -> impl Strategy<Value = Op> {
    prop_oneof![
        any::<bool>().prop_map(Op::Bool),
        any::<u8>().prop_map(Op::Nib),
        (0..=8u8, any::<u8>()).prop_map(|(b, v)| Op::Un8(b, v)),
        (0..=16u8, any::<u16>()).prop_map(|(b, v)| Op::Un16(b, v)),
        (0..=32u8, any::<u32>()).prop_map(|(b, v)| Op::Un32(b, v)),
        (0..=64u8, any::<u64>()).prop_map(|(b, v)| Op::Un64(b, v)),
        any::<u8>().prop_map(Op::U8),
        any::<u16>().prop_map(Op::U16),
        any::<u32>().prop_map(Op::U32),
        any::<u64>().prop_map(Op::U64),
        any::<i8>().prop_map(Op::I8),
        any::<i16>().prop_map(Op::I16),
        any::<i32>().prop_map(Op::I32),
        any::<i64>().prop_map(Op::I64),
        any::<u32>().prop_map(Op::F32),
        any::<u64>().prop_map(Op::F64),
        prop_oneof![0..16u32, any::<u32>()].prop_map(Op::UNib32),
        prop_oneof![0..200u32, any::<u32>()].prop_map(Op::UVlq32),
        prop_oneof![0..200u64, any::<u64>()].prop_map(Op::UVlq64),
        proptest::collection::vec(any::<u8>(), 0..12).prop_map(Op::Raw),
        proptest::collection::vec(any::<u8>(), 0..12).prop_map(Op::Bytes),
        ".{0,6}".prop_map(Op::Str),
        any::<u16>().prop_map(Op::RevLen),
        Just(Op::AlignByte),
    ]
}

fn ops() -> impl Strategy<Value = Vec<Op>> {
    let op = leaf().prop_recursive(3, 48, 8, |inner| {
        prop_oneof![
            4 => leaf(),
            1 => proptest::collection::vec(inner, 0..8).prop_map(Op::Unsized),
        ]
    });
    proptest::collection::vec(op, 0..16)
}

const BIG: usize = 4096;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2000))]

    /// Same bytes from both writers, read back by both readers to what was written.
    #[test]
    fn round_trip(ops in ops()) {
        let bytes = check_writes(&ops, BIG).expect("fits");
        let mut want = Vec::new();
        expected(&ops, &mut want);
        // Rust writes -> C reads (and Rust reads), check_reads compares the two
        let vals = check_reads(&bytes, &ops).expect("reads back");
        prop_assert_eq!(&vals, &want);
    }

    /// C writes -> Rust reads, on bytes the C writer produced on its own.
    #[test]
    fn c_writes_rust_reads(ops in ops()) {
        let bytes = shrink_wrap_c::model::c_bytes(&ops, BIG).expect("fits");
        let mut rd = BufReader::new(&bytes);
        let mut vals = Vec::new();
        shrink_wrap_c::model::rust_read(&mut rd, &ops, &mut vals).expect("Rust reads C bytes");
        let mut want = Vec::new();
        expected(&ops, &mut want);
        prop_assert_eq!(vals, want);
    }

    /// A buffer too small: the same first error on both sides, finish() returns it.
    #[test]
    fn writes_out_of_space(ops in ops(), cut in any::<prop::sample::Index>()) {
        let full = check_writes(&ops, BIG).expect("fits").len();
        // the lengths at the back need room before they are encoded, so a cap a bit above the size fails too
        let cap = cut.index(full + 8);
        let _ = check_writes(&ops, cap);
    }

    /// Truncated input: values up to the first error, then the latch.
    #[test]
    fn reads_truncated(ops in ops(), cut in any::<prop::sample::Index>()) {
        let bytes = check_writes(&ops, BIG).expect("fits");
        let len = if bytes.is_empty() { 0 } else { cut.index(bytes.len()) };
        let _ = check_reads(&bytes[..len], &ops);
    }

    /// Corrupted input: whatever the bytes say, both sides agree.
    #[test]
    fn reads_corrupted(ops in ops(), flips in proptest::collection::vec((any::<prop::sample::Index>(), 1..=255u8), 1..4)) {
        let mut bytes = check_writes(&ops, BIG).expect("fits");
        if !bytes.is_empty() {
            for (idx, x) in flips {
                let i = idx.index(bytes.len());
                bytes[i] ^= x;
            }
        }
        let _ = check_reads(&bytes, &ops);
    }

    /// Random bytes read as random ops.
    #[test]
    fn reads_random(bytes in proptest::collection::vec(any::<u8>(), 0..64), ops in ops()) {
        let _ = check_reads(&bytes, &ops);
    }

    /// The Rust `_latch` reads (what derived code uses) agree with the C reader all the way, also after an error.
    #[test]
    fn latch_reads(bytes in proptest::collection::vec(any::<u8>(), 0..24), reads in proptest::collection::vec(0..12u8, 0..24)) {
        let mut rd = BufReader::new(&bytes);
        let mut crd = CReader::new(&bytes);
        for r in reads {
            let (rust, c) = match r {
                0 => (rd.read_bool_latch() as u64, crd.read_bool() as u64),
                1 => (rd.read_u8_latch() as u64, crd.read_u8() as u64),
                2 => (rd.read_u16_latch() as u64, crd.read_u16() as u64),
                3 => (rd.read_u32_latch() as u64, crd.read_u32() as u64),
                4 => (rd.read_i8_latch() as u64, crd.read_i8() as u64),
                5 => (rd.read_i16_latch() as u64, crd.read_i16() as u64),
                6 => (rd.read_i32_latch() as u64, crd.read_i32() as u64),
                7 => (rd.read_f32_latch().to_bits() as u64, crd.read_f32().to_bits() as u64),
                8 => (rd.read_un8_latch(3) as u64, crd.read_un8(3) as u64),
                9 => (rd.read_un16_latch(13) as u64, crd.read_un16(13) as u64),
                10 => (rd.read_un32_latch(29) as u64, crd.read_un32(29) as u64),
                _ => (rd.read_un64_latch(61), crd.read_un64(61)),
            };
            prop_assert_eq!(rust, c);
        }
        let rust_err = rd.latched().err().map_or(0, shrink_wrap_c::code);
        prop_assert_eq!(rust_err, crd.error());
    }
}

/// The op model counts one value per read, also for nested `Unsized` values.
#[test]
fn read_count_nested() {
    let ops = [
        Op::U8(1),
        Op::AlignByte,
        Op::Unsized(vec![Op::Bool(true), Op::Unsized(vec![Op::Str("a".into())])]),
    ];
    assert_eq!(read_count(&ops), 3);
    let mut want = Vec::new();
    expected(&ops, &mut want);
    assert_eq!(want, [Val::N(1), Val::N(1), Val::B(b"a".to_vec())]);
}

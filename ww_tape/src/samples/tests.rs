use super::*;

/// xorshift64*, a fixed seed per test.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
}

fn encode_vec<T: Sample>(codec: Codec, shape: SampleShape, values: &[T]) -> Vec<u8> {
    let lanes = shape.lanes().unwrap() as usize;
    let mut out = vec![0u8; max_encoded_len(codec, shape, values.len() / lanes).unwrap()];
    let n = encode(codec, shape, values, &mut out).unwrap();
    out.truncate(n);
    out
}

/// Encode, check the column, decode the whole and a few ranges back.
fn roundtrip<T: Sample>(codec: Codec, shape: SampleShape, values: &[T]) -> Vec<u8> {
    let lanes = shape.lanes().unwrap() as usize;
    let count = values.len() / lanes;
    let bytes = encode_vec(codec, shape, values);
    let s = Samples::new(codec, shape, count, &bytes).unwrap();
    let mut out = vec![T::default(); values.len()];
    s.decode(0, &mut out).unwrap();
    assert!(
        out.iter()
            .zip(values)
            .all(|(a, b)| a.to_bits() == b.to_bits()),
        "{codec:?} {shape:?}"
    );
    for (first, n) in [
        (0, 1),
        (count / 3, count / 2),
        (count.saturating_sub(5), 5),
        (1023, 3),
        (1020, 1030),
    ] {
        if first + n > count || n == 0 {
            continue;
        }
        let mut part = vec![T::default(); n * lanes];
        s.decode(first, &mut part).unwrap();
        assert!(
            part.iter()
                .zip(&values[first * lanes..])
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "{codec:?} range {first}+{n}"
        );
    }
    bytes
}

#[test]
fn packed12_bytes() {
    let shape = SampleShape::scalar(Element::U16);
    let v: [u16; 3] = [0xabc, 0x123, 0xfff];
    assert_eq!(
        encode_vec(Codec::PACKED12_LSB, shape, &v),
        [0xbc, 0x3a, 0x12, 0xff, 0x0f]
    );
    assert_eq!(
        encode_vec(Codec::PACKED12_MSB, shape, &v),
        [0xab, 0xc1, 0x23, 0xff, 0xf0]
    );
    let s: [i16; 2] = [-1, -2048];
    assert_eq!(
        encode_vec(Codec::PACKED12_LSB, SampleShape::scalar(Element::I16), &s),
        [0xff, 0x0f, 0x80]
    );
    roundtrip(Codec::PACKED12_LSB, SampleShape::scalar(Element::I16), &s);
    roundtrip(Codec::PACKED12_MSB, SampleShape::scalar(Element::I16), &s);
}

#[test]
fn packed24_and_big_endian_bytes() {
    let v: [i32; 2] = [-2, 0x123456];
    let shape = SampleShape::scalar(Element::I32);
    assert_eq!(
        encode_vec(Codec::PACKED24_LSB, shape, &v),
        [0xfe, 0xff, 0xff, 0x56, 0x34, 0x12]
    );
    assert_eq!(
        encode_vec(Codec::PACKED24_MSB, shape, &v),
        [0xff, 0xff, 0xfe, 0x12, 0x34, 0x56]
    );
    assert_eq!(
        encode_vec(Codec::BIG_ENDIAN, shape, &v[1..]),
        [0x00, 0x12, 0x34, 0x56]
    );
    assert_eq!(
        encode_vec(Codec::RAW, shape, &v[1..]),
        [0x56, 0x34, 0x12, 0x00]
    );
    let b = [true, false, true, true, false, false, false, false, true];
    assert_eq!(
        encode_vec(Codec::RAW, SampleShape::scalar(Element::Bool), &b),
        [0b0000_1101, 0b1]
    );
    roundtrip(Codec::RAW, SampleShape::scalar(Element::Bool), &b);
}

#[test]
fn packed_refuses_wide_values() {
    let shape = SampleShape::scalar(Element::I16);
    let mut out = [0u8; 16];
    assert_eq!(
        encode(Codec::PACKED12_LSB, shape, &[0i16, 2048], &mut out),
        Err(SampleError::ValueTooWide { index: 1 })
    );
    assert_eq!(
        encode(
            Codec::PACKED10_MSB,
            SampleShape::scalar(Element::U16),
            &[1024u16],
            &mut out
        ),
        Err(SampleError::ValueTooWide { index: 0 })
    );
}

#[test]
fn every_raw_format_round_trips() {
    let mut r = Rng(1);
    for n in [1usize, 2, 3, 7, 8, 9, 100, 4099] {
        let u16s: Vec<u16> = (0..n).map(|_| r.next() as u16).collect();
        let i64s: Vec<i64> = (0..n).map(|_| r.next() as i64).collect();
        let f32s: Vec<f32> = (0..n).map(|_| f32::from_bits(r.next() as u32)).collect();
        for c in [Codec::RAW, Codec::BIG_ENDIAN] {
            roundtrip(c, SampleShape::scalar(Element::U16), &u16s);
            roundtrip(c, SampleShape::scalar(Element::I64), &i64s);
            roundtrip(c, SampleShape::scalar(Element::F32), &f32s);
        }
        for (c, w) in [
            (Codec::PACKED10_LSB, 10),
            (Codec::PACKED10_MSB, 10),
            (Codec::PACKED12_LSB, 12),
            (Codec::PACKED12_MSB, 12),
            (Codec::PACKED24_LSB, 24),
            (Codec::PACKED24_MSB, 24),
        ] {
            let signed: Vec<i32> = (0..n)
                .map(|_| ((r.next() as i64) >> (64 - w)) as i32)
                .collect();
            let unsigned: Vec<u64> = (0..n).map(|_| r.next() >> (64 - w)).collect();
            roundtrip(c, SampleShape::scalar(Element::I32), &signed);
            roundtrip(c, SampleShape::scalar(Element::U64), &unsigned);
            if w < 16 {
                let iq: Vec<i16> = (0..2 * n)
                    .map(|_| ((r.next() as i64) >> (64 - w)) as i16)
                    .collect();
                roundtrip(c, SampleShape::scalar(Element::ComplexI16), &iq);
            }
        }
    }
}

/// A slow sine with a little noise, as a sensor gives, in `f64`.
fn sensor(n: usize, seed: u64) -> Vec<f64> {
    let mut r = Rng(seed);
    (0..n)
        .map(|i| {
            let noise = (r.next() >> 54) as f64 / 1024.0;
            ((i as f64 / 200.0).sin() * 100.0 + noise * 0.01 * 100.0).round() / 100.0
        })
        .collect()
}

#[test]
fn xor_codecs_round_trip_every_pattern() {
    let mut r = Rng(7);
    for c in [Codec::GORILLA, Codec::CHIMP] {
        for n in [1usize, 2, 1023, 1024, 1025, 3000] {
            let any64: Vec<f64> = (0..n).map(|_| f64::from_bits(r.next())).collect();
            let any32: Vec<f32> = (0..n).map(|_| f32::from_bits(r.next() as u32)).collect();
            roundtrip(c, SampleShape::scalar(Element::F64), &any64);
            roundtrip(c, SampleShape::scalar(Element::F32), &any32);
            let s = sensor(n, 3);
            roundtrip(c, SampleShape::scalar(Element::F64), &s);
            let s32: Vec<f32> = s.iter().map(|v| *v as f32).collect();
            roundtrip(c, SampleShape::scalar(Element::F32), &s32);
            let iq: Vec<f32> = (0..2 * n).map(|i| (i / 7) as f32 * 0.5).collect();
            roundtrip(c, SampleShape::scalar(Element::ComplexF32), &iq);
            let three: Vec<f64> = (0..3 * n).map(|i| [1.5, f64::NAN, -0.0][i % 3]).collect();
            roundtrip(c, SampleShape::new(Element::F64, 3), &three);
            let flat = vec![0.25f64; n];
            roundtrip(c, SampleShape::scalar(Element::F64), &flat);
        }
    }
}

#[test]
fn xor_codecs_compress_smooth_data() {
    let s = sensor(10_000, 11);
    for c in [Codec::GORILLA, Codec::CHIMP] {
        let bytes = roundtrip(c, SampleShape::scalar(Element::F64), &s);
        assert!(
            bytes.len() * 2 < s.len() * 8,
            "{c:?}: {} bytes",
            bytes.len()
        );
        let flat = vec![3.0f64; 10_000];
        let bytes = roundtrip(c, SampleShape::scalar(Element::F64), &flat);
        assert!(
            bytes.len() < 10_000 / 3,
            "{c:?}: {} bytes for a constant",
            bytes.len()
        );
    }
}

#[test]
fn groups_decode_alone() {
    let s = sensor(2500, 5);
    let shape = SampleShape::scalar(Element::F64);
    let mut bytes = encode_vec(Codec::GORILLA, shape, &s);
    let col = Samples::new(Codec::GORILLA, shape, s.len(), &bytes).unwrap();
    assert_eq!(col.group_count(), 3);
    let (samples, range) = col.group_bytes(1);
    assert_eq!(samples, 1024..2048);
    // damage the middle group: the others still decode
    for b in &mut bytes[range.clone()] {
        *b = 0xff;
    }
    let col = Samples::new(Codec::GORILLA, shape, s.len(), &bytes).unwrap();
    let mut out = vec![0f64; 1024];
    col.decode(0, &mut out).unwrap();
    assert_eq!(out, s[..1024]);
    let mut out = vec![0f64; 2500 - 2048];
    col.decode(2048, &mut out).unwrap();
    assert_eq!(out, s[2048..]);
    let mut out = vec![0f64; 1024];
    assert!(col.decode(1024, &mut out).is_err() || out != s[1024..2048]);
}

#[test]
fn columns_are_checked() {
    let shape = SampleShape::scalar(Element::U16);
    assert_eq!(
        Samples::new(Codec::PACKED12_LSB, shape, 3, &[0; 4]).unwrap_err(),
        SampleError::BadLength { len: 4 }
    );
    assert!(matches!(
        Samples::new(Codec::GORILLA, shape, 3, &[]).unwrap_err(),
        SampleError::Unsupported { .. }
    ));
    assert!(matches!(
        Samples::new(Codec::PACKED24_LSB, shape, 3, &[0; 9]).unwrap_err(),
        SampleError::Unsupported { .. }
    ));
    assert!(matches!(
        Samples::new(Codec(47), shape, 3, &[0; 6]).unwrap_err(),
        SampleError::Unsupported { .. }
    ));
    assert_eq!(
        Samples::new(Codec::RAW, SampleShape::new(Element::U16, 0), 3, &[0; 6]).unwrap_err(),
        SampleError::BadShape
    );
    let s = Samples::new(Codec::RAW, shape, 3, &[0; 6]).unwrap();
    assert!(matches!(
        s.decode(0, &mut [0i16; 3]),
        Err(SampleError::WrongType { .. })
    ));
    assert_eq!(s.decode(2, &mut [0u16; 2]), Err(SampleError::OutOfRange));
    assert_eq!(s.sample_bytes(1..3), Some(2..6));
    let f = SampleShape::scalar(Element::F32);
    // a table whose last end isn't the stream's length
    assert!(Samples::new(Codec::CHIMP, f, 1, &[9, 0, 0, 0, 1]).is_err());
    // a truncated stream reads as corrupt, not a panic
    let bytes = encode_vec(Codec::CHIMP, f, &[1.0f32, 2.0, 3.0]);
    let mut cut = bytes[..bytes.len() - 1].to_vec();
    cut[..4].copy_from_slice(&((bytes.len() - 5) as u32).to_le_bytes());
    let s = Samples::new(Codec::CHIMP, f, 3, &cut).unwrap();
    assert_eq!(
        s.decode(0, &mut [0f32; 3]),
        Err(SampleError::Corrupt { group: 0 })
    );
}

#[test]
fn deterministic() {
    let s = sensor(5000, 9);
    for c in [Codec::GORILLA, Codec::CHIMP, Codec::BIG_ENDIAN] {
        let shape = SampleShape::scalar(Element::F64);
        assert_eq!(encode_vec(c, shape, &s), encode_vec(c, shape, &s));
    }
}

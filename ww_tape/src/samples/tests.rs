use super::*;
use crate::tests::id;

/// xorshift64*, a fixed seed per test.
pub(super) struct Rng(pub(super) u64);

impl Rng {
    pub(super) fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
}

pub(super) fn encode_vec<T: Sample>(codec: Codec, shape: SampleShape, values: &[T]) -> Vec<u8> {
    let lanes = shape.lanes().unwrap() as usize;
    let mut out = vec![0u8; max_encoded_len(codec, shape, values.len() / lanes).unwrap()];
    let n = encode(codec, shape, values, &mut out).unwrap();
    out.truncate(n);
    out
}

/// Encode, check the column, decode the whole and a few ranges back.
pub(super) fn roundtrip<T: Sample>(codec: Codec, shape: SampleShape, values: &[T]) -> Vec<u8> {
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
pub(super) fn sensor(n: usize, seed: u64) -> Vec<f64> {
    let mut r = Rng(seed);
    (0..n)
        .map(|i| {
            let noise = (r.next() >> 54) as f64 / 1024.0;
            ((i as f64 / 200.0).sin() * 100.0 + noise * 0.01 * 100.0).round() / 100.0
        })
        .collect()
}

#[test]
fn groups_decode_alone() {
    let s = sensor(2500, 5);
    let shape = SampleShape::scalar(Element::F64);
    let mut bytes = encode_vec(Codec::ALP, shape, &s);
    let col = Samples::new(Codec::ALP, shape, s.len(), &bytes).unwrap();
    assert_eq!(col.group_count(), 20);
    let (samples, range) = col.group_bytes(1);
    assert_eq!(samples, 128..256);
    // damage the middle group: the others still decode
    for b in &mut bytes[range.clone()] {
        *b = 0xff;
    }
    let col = Samples::new(Codec::ALP, shape, s.len(), &bytes).unwrap();
    let mut out = vec![0f64; 128];
    col.decode(0, &mut out).unwrap();
    assert_eq!(out, s[..128]);
    let mut out = vec![0f64; 2500 - 256];
    col.decode(256, &mut out).unwrap();
    assert_eq!(out, s[256..]);
    let mut out = vec![0f64; 128];
    assert!(col.decode(128, &mut out).is_err() || out != s[128..256]);
}

#[test]
fn columns_are_checked() {
    let shape = SampleShape::scalar(Element::U16);
    assert_eq!(
        Samples::new(Codec::PACKED12_LSB, shape, 3, &[0; 4]).unwrap_err(),
        SampleError::BadLength { len: 4 }
    );
    assert!(matches!(
        Samples::new(Codec::ALP, shape, 3, &[]).unwrap_err(),
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
    // an outer-coded column: no outer codec is known, whatever the inner one
    for inner in [Codec::RAW, Codec::BIG_ENDIAN] {
        assert!(matches!(
            Samples::new(Codec(inner.0 | Codec::OUTER), shape, 3, &[0; 6]).unwrap_err(),
            SampleError::Unsupported { .. }
        ));
    }
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
    assert!(Samples::new(Codec::ALP, f, 1, &[9, 0, 0, 0, 1]).is_err());
    // a truncated stream reads as corrupt, not a panic
    let bytes = encode_vec(Codec::ALP, f, &[1.0f32, 2.0, 3.0]);
    let mut cut = bytes[..bytes.len() - 1].to_vec();
    cut[..4].copy_from_slice(&((bytes.len() - 5) as u32).to_le_bytes());
    let s = Samples::new(Codec::ALP, f, 3, &cut).unwrap();
    assert_eq!(
        s.decode(0, &mut [0f32; 3]),
        Err(SampleError::Corrupt { group: 0 })
    );
}

#[test]
fn deterministic() {
    let s = sensor(5000, 9);
    for c in [Codec::ALP, Codec::BIG_ENDIAN] {
        let shape = SampleShape::scalar(Element::F64);
        assert_eq!(encode_vec(c, shape, &s), encode_vec(c, shape, &s));
    }
}

#[test]
fn delta_codecs_round_trip_every_pattern() {
    let mut r = Rng(13);
    for n in [1usize, 2, 3, 127, 128, 129, 300, 1000] {
        let any_i64: Vec<i64> = (0..n).map(|_| r.next() as i64).collect();
        let any_u8: Vec<u8> = (0..n).map(|_| r.next() as u8).collect();
        let ramp: Vec<i32> = (0..n as i32).map(|i| i * 7 - 100).collect();
        let iq: Vec<i16> = (0..2 * n)
            .map(|i| ((i as f64 / 9.0).sin() * 2000.0) as i16)
            .collect();
        let edges: Vec<i16> = (0..n).map(|i| [i16::MIN, i16::MAX, 0, -1][i % 4]).collect();
        for c in [Codec::DELTA, Codec::DELTA2] {
            roundtrip(c, SampleShape::scalar(Element::I64), &any_i64);
            roundtrip(c, SampleShape::scalar(Element::U8), &any_u8);
            roundtrip(c, SampleShape::scalar(Element::I32), &ramp);
            roundtrip(c, SampleShape::scalar(Element::ComplexI16), &iq);
            roundtrip(c, SampleShape::new(Element::I16, 4), &edges[..n / 4 * 4]);
        }
    }
}

#[test]
fn delta_bytes() {
    // a ramp packs at width 0: first value, min, width 0, no packed bytes
    let ramp: Vec<u16> = (0..5).map(|i| 10 + 3 * i).collect();
    assert_eq!(
        encode_vec(Codec::DELTA, SampleShape::scalar(Element::U16), &ramp),
        [5, 0, 0, 0, 10, 0, 3, 0, 0]
    );
    // order 2 of a parabola: first, first difference, min of second differences
    let p: Vec<i16> = (0..4).map(|i| i * i).collect();
    assert_eq!(
        encode_vec(Codec::DELTA2, SampleShape::scalar(Element::I16), &p),
        [7, 0, 0, 0, 0, 0, 1, 0, 2, 0, 0]
    );
    // differences 1, -1, 2: min -1, residuals 2, 0, 3 at width 2
    let v: Vec<i8> = vec![0, 1, 0, 2];
    assert_eq!(
        encode_vec(Codec::DELTA, SampleShape::scalar(Element::I8), &v),
        [4, 0, 0, 0, 0, 0xff, 2, 0b11_00_10]
    );
}

#[test]
fn delta_rejects_malformed_groups() {
    let shape = SampleShape::scalar(Element::I16);
    let v: Vec<i16> = (0..200).map(|i| (i * i % 300) as i16).collect();
    let bytes = encode_vec(Codec::DELTA, shape, &v);
    // a width over the scalar's bits
    let mut bad = bytes.clone();
    bad[8 + 4] = 17;
    let s = Samples::new(Codec::DELTA, shape, 200, &bad).unwrap();
    assert_eq!(
        s.decode(0, &mut [0i16; 200]),
        Err(SampleError::Corrupt { group: 0 })
    );
    // the second group still decodes
    let mut tail = [0i16; 72];
    s.decode(128, &mut tail).unwrap();
    assert_eq!(tail, v[128..]);
    assert!(Samples::new(Codec::DELTA, SampleShape::scalar(Element::F32), 1, &[]).is_err());
}

/// The ids of the float codecs that left the format stay reserved: refused for every element, on read and on write.
#[test]
fn reserved_float_codec_ids_are_refused() {
    for id in [24, 25, 34, 35] {
        for element in [
            Element::F32,
            Element::F64,
            Element::ComplexF32,
            Element::I32,
        ] {
            let shape = SampleShape::scalar(element);
            assert!(matches!(
                Samples::new(Codec(id), shape, 1, &[0; 16]),
                Err(SampleError::Unsupported { .. })
            ));
            assert!(matches!(
                max_encoded_len(Codec(id), shape, 1),
                Err(SampleError::Unsupported { .. })
            ));
        }
    }
}

/// Columns of every codec, and a series chunk holding some, are the golden file's (`WW_TAPE_GOLDEN=update` rewrites
/// it): one line per column, `codec element channels count: hex`, then the chunk block. Inputs come from integer
/// arithmetic only, so they are the same on every platform.
#[test]
fn samples_golden() {
    let mut r = Rng(2026);
    let n = 300;
    let wave: Vec<i64> = (0..n as i64)
        .map(|i| (i * i * 37 % 4001) - 2000 + i % 7)
        .collect();
    let i16s: Vec<i16> = wave.iter().map(|v| *v as i16).collect();
    let u16s: Vec<u16> = wave.iter().map(|v| (*v + 2000) as u16).collect();
    let i32s: Vec<i32> = wave.iter().map(|v| (*v * 1000) as i32).collect();
    let f64s: Vec<f64> = wave.iter().map(|v| *v as f64 / 16.0).collect();
    let f32s: Vec<f32> = wave.iter().map(|v| *v as f32 / 8.0).collect();
    let any: Vec<f64> = (0..n).map(|_| f64::from_bits(r.next())).collect();
    let bools: Vec<bool> = wave.iter().map(|v| v % 3 == 0).collect();
    let mut lines = vec![];
    let mut add = |line: String| lines.push(line);
    fn col<T: Sample>(c: Codec, shape: SampleShape, v: &[T]) -> String {
        let bytes = roundtrip(c, shape, v);
        let lanes = shape.lanes().unwrap() as usize;
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        format!(
            "{} {:?} {} {}: {hex}",
            c.0,
            shape.element,
            shape.channels.0,
            v.len() / lanes
        )
    }
    let s = SampleShape::scalar;
    add(col(Codec::RAW, s(Element::I16), &i16s));
    add(col(Codec::RAW, s(Element::Bool), &bools));
    add(col(Codec::BIG_ENDIAN, s(Element::I32), &i32s));
    add(col(
        Codec::BIG_ENDIAN,
        SampleShape::new(Element::ComplexI16, 1),
        &i16s,
    ));
    for c in [Codec::PACKED12_LSB, Codec::PACKED12_MSB] {
        add(col(c, s(Element::I16), &i16s));
        add(col(c, s(Element::U16), &u16s));
    }
    let tens: Vec<i16> = i16s.iter().map(|v| v / 4).collect();
    add(col(Codec::PACKED10_LSB, s(Element::I16), &tens));
    add(col(Codec::PACKED10_MSB, s(Element::I16), &tens));
    add(col(Codec::PACKED24_LSB, s(Element::I32), &i32s));
    add(col(Codec::PACKED24_MSB, s(Element::I32), &i32s));
    add(col(Codec::ALP, s(Element::F64), &f64s));
    add(col(Codec::ALP, s(Element::F32), &f32s));
    add(col(Codec::ALP, s(Element::F64), &any[..40]));
    add(col(
        Codec::ALP,
        SampleShape::new(Element::ComplexF32, 1),
        &f32s,
    ));
    // decimals, the data ALP codes as integers: IEEE division gives the same values everywhere
    let cents: Vec<f64> = wave.iter().map(|v| *v as f64 / 100.0).collect();
    let cents32: Vec<f32> = wave.iter().map(|v| *v as f32 / 100.0).collect();
    add(col(Codec::ALP, s(Element::F64), &cents));
    add(col(Codec::ALP, s(Element::F32), &cents32));
    for c in [Codec::DELTA, Codec::DELTA2] {
        add(col(c, s(Element::I16), &i16s));
        add(col(c, SampleShape::new(Element::I32, 3), &i32s));
        add(col(c, s(Element::I64), &wave));
    }
    let lz4 = {
        let raw: Vec<u8> = i16s.iter().flat_map(|v| v.to_le_bytes()).collect();
        let mut out = vec![0u8; crate::lz4::lz4_max_len(raw.len())];
        let n = crate::lz4::lz4_encode(&raw, &mut out).unwrap();
        out.truncate(n);
        out
    };
    lines.push(format!(
        "48 payload: {}",
        lz4.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ));
    // a series chunk: channel 0 packed 12-bit, channel 1 delta
    let shape = s(Element::I16);
    let c0 = encode_vec(Codec::PACKED12_LSB, shape, &i16s);
    let c1 = encode_vec(Codec::DELTA, shape, &i16s);
    let head = crate::ChunkHead {
        kind: crate::BlockKind::Series,
        track_id: 0x5a5a,
        track_number: 3,
        first_item: 1000,
        count: n as u32,
        first_tick: 0,
        last_tick: (n as i64 - 1) * 20,
        latest_end: (n as i64 - 1) * 20,
        time_mode: 0,
        end_mode: crate::EndMode::Absent,
    };
    let spec = |codec, param, len| crate::ColumnSpec {
        kind: crate::ColumnKind::Samples,
        codec,
        param,
        len,
    };
    let cols = [
        (spec(Codec::PACKED12_LSB, 0, c0.len()), &c0[..]),
        (spec(Codec::DELTA, 1, c1.len()), &c1[..]),
    ];
    let specs: Vec<_> = cols.iter().map(|c| c.0).collect();
    let mut block = vec![0u8; crate::chunk::chunk_len(&specs, 0)];
    let len = crate::chunk::write_chunk(&mut block, id(1), &head, &cols, 0).unwrap();
    block.truncate(len);
    let read = crate::Block::read(&block).unwrap();
    let chunk = read.chunk().unwrap();
    for (param, codec) in [(0, Codec::PACKED12_LSB), (1, Codec::DELTA)] {
        let column = chunk.find(crate::ColumnKind::Samples, param).unwrap();
        assert_eq!(column.codec, codec);
        let samples = Samples::of(&column, shape, n).unwrap();
        let mut out = vec![0i16; n];
        samples.decode(0, &mut out).unwrap();
        assert_eq!(out, i16s);
    }
    lines.push(format!(
        "chunk: {}",
        block.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ));
    let text = lines.join("\n") + "\n";
    let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("golden/v2-samples.hex");
    if std::env::var("WW_TAPE_GOLDEN").as_deref() == Ok("update") {
        std::fs::write(&golden, &text).unwrap();
    }
    let want = std::fs::read_to_string(&golden)
        .expect("golden/v2-samples.hex: run with WW_TAPE_GOLDEN=update");
    assert!(
        text == want,
        "the format changed: bytes differ from golden/v2-samples.hex"
    );
}

/// Random bytes as columns of every codec and element: a check or a decode may fail, never panic (the fuzz target
/// `tape-samples` goes further).
#[test]
fn random_columns_never_panic() {
    let mut r = Rng(99);
    let codecs = [
        Codec::RAW,
        Codec::BIG_ENDIAN,
        Codec::PACKED10_LSB,
        Codec::PACKED12_MSB,
        Codec::PACKED24_LSB,
        Codec::DELTA,
        Codec::DELTA2,
        Codec::ALP,
    ];
    fn try_all<T: Sample>(codec: Codec, shape: SampleShape, count: usize, bytes: &[u8]) {
        if let Ok(s) = Samples::new(codec, shape, count, bytes) {
            let mut out = vec![T::default(); count * s.lanes()];
            let _ = s.decode(0, &mut out);
        }
    }
    for round in 0..2000 {
        let len = (r.next() % 600) as usize;
        let mut bytes: Vec<u8> = (0..len).map(|_| r.next() as u8).collect();
        let count = (r.next() % 300) as usize;
        let groups = if round % 2 == 0 {
            count.div_ceil(DELTA_GROUP)
        } else {
            count.div_ceil(ALP_GROUP)
        };
        // a plausible table half the time, so the group decoders run
        if round % 4 < 2 && bytes.len() >= groups * 4 && groups > 0 {
            let data = (bytes.len() - groups * 4) as u32;
            for g in 0..groups {
                let end = data * (g as u32 + 1) / groups as u32;
                bytes[g * 4..g * 4 + 4].copy_from_slice(&end.to_le_bytes());
            }
        }
        let codec = codecs[round % codecs.len()];
        let channels = 1 + (round / codecs.len()) as u32 % 3;
        try_all::<i16>(
            codec,
            SampleShape::new(Element::I16, channels),
            count,
            &bytes,
        );
        try_all::<u32>(
            codec,
            SampleShape::new(Element::U32, channels),
            count,
            &bytes,
        );
        try_all::<i64>(
            codec,
            SampleShape::new(Element::I64, channels),
            count,
            &bytes,
        );
        try_all::<f32>(
            codec,
            SampleShape::new(Element::ComplexF32, channels),
            count,
            &bytes,
        );
        try_all::<f64>(
            codec,
            SampleShape::new(Element::F64, channels),
            count,
            &bytes,
        );
    }
}

#[test]
fn grouped_columns_stay_under_4_gib() {
    // 2^29 u64 values are 4 GiB of data, past what a table end addresses: refused, not a panic in encode
    let s = SampleShape::scalar(Element::U64);
    for c in [Codec::DELTA, Codec::DELTA2] {
        assert_eq!(max_encoded_len(c, s, 1 << 29), Err(SampleError::BadShape));
        assert!(max_encoded_len(c, s, 1 << 20).is_ok());
    }
}

#[test]
fn raw_ranges_past_the_bytes_are_errors() {
    let short = [0u8; 3];
    assert_eq!(
        raw::decode_le(&short, 1, &mut [0u16; 1]),
        Err(SampleError::OutOfRange)
    );
    assert_eq!(
        raw::decode_be(&short, usize::MAX, &mut [0u16; 1]),
        Err(SampleError::OutOfRange)
    );
    let mut out = [0u16; 3];
    for packing in [raw::Packing::Lsb(12), raw::Packing::Msb12] {
        assert_eq!(
            raw::decode_packed(&short, packing, false, usize::MAX - 1, &mut out),
            Err(SampleError::OutOfRange),
            "{packing:?}"
        );
    }
    assert_eq!(
        raw::decode_packed(&short, raw::Packing::Msb12, false, 2, &mut out[..1]),
        Err(SampleError::OutOfRange)
    );
}

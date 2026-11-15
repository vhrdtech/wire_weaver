use super::*;
use crate::Codec;
use crate::samples::tests::{Rng, encode_vec, roundtrip, sensor};
use crate::samples::{Element, SampleShape, Samples, max_encoded_len};

const F64S: SampleShape = SampleShape::scalar(Element::F64);
const F32S: SampleShape = SampleShape::scalar(Element::F32);

/// Edge patterns: signed zeros, infinities, NaN payloads, subnormals, extremes, and decimals around them.
fn edges64() -> Vec<f64> {
    let mut v = vec![
        0.0,
        -0.0,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NAN,
        f64::from_bits(0x7ff0_0000_0000_0001),
        f64::from_bits(0xfff8_dead_beef_0001),
        f64::MIN_POSITIVE,
        f64::from_bits(1),
        f64::MAX,
        f64::MIN,
        f64::EPSILON,
        1e300,
        -1e-300,
        2f64.powi(51),
        -(2f64.powi(51)),
        9.007_199_254_740_993e15,
    ];
    v.extend((0..200).map(|i| f64::from(i) * 0.1 - 3.0));
    v
}

#[test]
fn round_trips_every_pattern() {
    let mut r = Rng(31);
    for n in [1usize, 2, 127, 128, 129, 1000, 3000] {
        let any64: Vec<f64> = (0..n).map(|_| f64::from_bits(r.next())).collect();
        let any32: Vec<f32> = (0..n).map(|_| f32::from_bits(r.next() as u32)).collect();
        roundtrip(Codec::ALP, F64S, &any64);
        roundtrip(Codec::ALP, F32S, &any32);
        let s = sensor(n, 3);
        roundtrip(Codec::ALP, F64S, &s);
        let s32: Vec<f32> = s.iter().map(|v| *v as f32).collect();
        roundtrip(Codec::ALP, F32S, &s32);
        let iq: Vec<f32> = (0..2 * n).map(|i| (i / 7) as f32 * 0.5).collect();
        roundtrip(Codec::ALP, SampleShape::scalar(Element::ComplexF32), &iq);
        let three: Vec<f64> = (0..3 * n).map(|i| [1.5, f64::NAN, -0.0][i % 3]).collect();
        roundtrip(Codec::ALP, SampleShape::new(Element::F64, 3), &three);
        roundtrip(Codec::ALP, F64S, &vec![0.25f64; n]);
        // more lanes than the encoder keeps plans for
        let wide: Vec<f64> = (0..11 * n).map(|i| (i % 97) as f64 / 8.0).collect();
        roundtrip(Codec::ALP, SampleShape::new(Element::F64, 11), &wide);
    }
    let e = edges64();
    roundtrip(Codec::ALP, F64S, &e);
    let e32: Vec<f32> = e.iter().map(|v| *v as f32).collect();
    roundtrip(Codec::ALP, F32S, &e32);
    // the same values with mixed decimal and real lanes in one group
    let mixed: Vec<f64> = (0..600)
        .map(|i| {
            if i % 9 == 0 {
                f64::from_bits(r.next())
            } else {
                f64::from(i) / 1000.0
            }
        })
        .collect();
    roundtrip(Codec::ALP, F64S, &mixed);
}

/// The scheme byte of the first lane of group 0.
fn first_scheme(bytes: &[u8], count: usize) -> u8 {
    bytes[count.div_ceil(ALP_GROUP) * 4]
}

#[test]
fn decimals_pack_as_integers() {
    let s = sensor(10_000, 11);
    let bytes = roundtrip(Codec::ALP, F64S, &s);
    assert_eq!(first_scheme(&bytes, s.len()), DECIMAL);
    // two decimals of a sine of amplitude 100: 15 bits a value
    assert!(bytes.len() * 4 < s.len() * 8, "{} bytes", bytes.len());
    let s32: Vec<f32> = s.iter().map(|v| *v as f32).collect();
    let bytes = roundtrip(Codec::ALP, F32S, &s32);
    assert_eq!(first_scheme(&bytes, s.len()), DECIMAL);
    assert!(bytes.len() * 2 < s.len() * 4, "{} bytes", bytes.len());
    // a constant: the table entry and an rd lane head whose dictionary holds the value, no bits per value
    let flat = vec![3.0f64; 10_000];
    let bytes = roundtrip(Codec::ALP, F64S, &flat);
    assert_eq!(bytes.len(), 10_000usize.div_ceil(ALP_GROUP) * (4 + 7));
}

#[test]
fn real_doubles_take_rd_and_never_grow() {
    let mut r = Rng(5);
    // doubles in [1, 2): the sign and exponent are the same, the mantissa is noise
    let real: Vec<f64> = (0..5000)
        .map(|_| f64::from_bits(0x3ff0_0000_0000_0000 | (r.next() >> 12)))
        .collect();
    let bytes = roundtrip(Codec::ALP, F64S, &real);
    assert_eq!(first_scheme(&bytes, real.len()), RD);
    assert!(bytes.len() < real.len() * 8, "{} bytes", bytes.len());
    let any: Vec<f64> = (0..5000).map(|_| f64::from_bits(r.next())).collect();
    let bytes = roundtrip(Codec::ALP, F64S, &any);
    let groups = any.len().div_ceil(ALP_GROUP);
    assert!(bytes.len() <= any.len() * 8 + groups * (4 + 7));
    assert!(bytes.len() <= max_encoded_len(Codec::ALP, F64S, any.len()).unwrap());
}

#[test]
fn deterministic_and_alone_per_group() {
    let s = sensor(1000, 9);
    let a = encode_vec(Codec::ALP, F64S, &s);
    assert_eq!(a, encode_vec(Codec::ALP, F64S, &s));
    let col = Samples::new(Codec::ALP, F64S, s.len(), &a).unwrap();
    assert_eq!(col.group_count(), 8);
    let (samples, range) = col.group_bytes(3).unwrap();
    assert_eq!(samples, 384..512);
    let mut damaged = a.clone();
    for b in &mut damaged[range] {
        *b = 0xee;
    }
    let col = Samples::new(Codec::ALP, F64S, s.len(), &damaged).unwrap();
    let mut out = vec![0f64; 384];
    col.decode(0, &mut out).unwrap();
    assert_eq!(out, s[..384]);
    let mut out = vec![0f64; 1000 - 512];
    col.decode(512, &mut out).unwrap();
    assert_eq!(out, s[512..]);
}

#[test]
fn bytes_of_a_small_column() {
    // 0.1 to 0.4 and an exception (-0.0): e 1, f 0, base 1, residuals 0 to 3 at width 2
    let v = [0.1f32, 0.2, 0.3, 0.4, -0.0];
    let bytes = encode_vec(Codec::ALP, F32S, &v);
    let mut want = vec![];
    want.extend(16u32.to_le_bytes());
    want.extend([DECIMAL, 1, 0]);
    want.extend(1i32.to_le_bytes());
    want.extend([2, 1]);
    // residuals 0, 1, 2, 3, 0 (the exception) at 2 bits, LSB first
    want.extend([0b1110_0100, 0]);
    want.push(4);
    want.extend((-0.0f32).to_bits().to_le_bytes());
    assert_eq!(bytes, want);
    roundtrip(Codec::ALP, F32S, &v);
}

#[test]
fn rd_drops_shared_zero_bits() {
    // f32 values stored as f64: 29 zero bits at the bottom of every value
    let s: Vec<f64> = sensor(1000, 4)
        .iter()
        .map(|v| f64::from(*v as f32 * 1.37))
        .collect();
    let bytes = roundtrip(Codec::ALP, F64S, &s);
    assert_eq!(first_scheme(&bytes, s.len()), RD);
    assert!(bytes.len() * 2 < s.len() * 8, "{} bytes", bytes.len());
}

/// A one-group column of `body` (a lane of 4 `f32` samples).
fn column(body: &[u8]) -> Vec<u8> {
    let mut c = (body.len() as u32).to_le_bytes().to_vec();
    c.extend(body);
    c
}

fn decodes(body: &[u8]) -> bool {
    let c = column(body);
    let Ok(s) = Samples::new(Codec::ALP, F32S, 4, &c) else {
        return false;
    };
    let mut out = [0f32; 4];
    s.decode(0, &mut out).is_ok()
}

#[test]
fn rejects_malformed_groups() {
    // decimal: e 1, f 0, base 0, width 4, no exception, 4 residuals in 2 bytes
    let good = [DECIMAL, 1, 0, 0, 0, 0, 0, 4, 0, 0x21, 0x43];
    assert!(decodes(&good));
    let c = column(&good);
    let mut out = [0f32; 4];
    Samples::new(Codec::ALP, F32S, 4, &c)
        .unwrap()
        .decode(0, &mut out)
        .unwrap();
    assert_eq!(out, [0.1, 0.2, 0.3, 0.4]);
    let bad = |at: usize, v: u8| {
        let mut b = good;
        b[at] = v;
        b
    };
    assert!(!decodes(&bad(0, 2)), "unknown scheme");
    assert!(!decodes(&bad(2, 2)), "f over e");
    assert!(!decodes(&bad(1, 11)), "e past the f32 table");
    assert!(!decodes(&bad(7, 33)), "width over the scalar");
    assert!(!decodes(&good[..10]), "short");
    assert!(!decodes(&[&good[..], &[0]].concat()), "trailing byte");
    // exceptions: positions ascending and in the group
    let exc = |p: [u8; 2]| {
        let mut b = good.to_vec();
        b[8] = 2;
        b.extend(p);
        b.extend([0u8; 8]);
        b
    };
    assert!(decodes(&exc([0, 3])));
    assert!(!decodes(&exc([3, 0])), "descending");
    assert!(!decodes(&exc([1, 1])), "repeated");
    assert!(!decodes(&exc([0, 4])), "past the group");
    assert!(!decodes(&[good[0], 1, 0]), "cut in the header");
    // rd: left 4 bits, no zeros, a one-entry dictionary, codes of 0 bits, right parts of 28 bits
    let mut rd = vec![RD, 4, 0, 0, 0x03, 0x00, 0];
    rd.extend([0u8; 14]);
    assert!(decodes(&rd));
    let mut wide = rd.clone();
    wide[4] = 0x10;
    assert!(!decodes(&wide), "dictionary entry over the left bits");
    let mut left = rd.clone();
    left[1] = 17;
    assert!(!decodes(&left), "left part over 16 bits");
    let mut code = rd.clone();
    code[3] = 4;
    assert!(!decodes(&code), "dictionary over 8 entries");
    let mut zeros = rd.clone();
    zeros[2] = 29;
    assert!(!decodes(&zeros), "zeros over the right part");
    // 4 zeros: the right parts take 24 bits, 12 bytes
    let mut shifted = rd.clone();
    shifted[2] = 4;
    shifted.truncate(shifted.len() - 2);
    assert!(decodes(&shifted));
}

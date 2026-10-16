#![no_main]

//! `Series<T>` column codecs (SW-60): random numeric (1 to 64 and 128 bits), float, tag / bool / `Option` and
//! opaque run columns round-trip through every codec and the codec choice, and every column reader on random bytes
//! returns an error or values, never a panic or a read past its bytes. The checks themselves are in
//! `shrink_wrap::series::fuzz_columns` (public under `cargo fuzz`'s `--cfg fuzzing`), shared with the unit tests.

use arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use shrink_wrap::series::fuzz_columns as check;
use shrink_wrap::series::fuzz_columns::{Coder, FloatCodec, NumericCodec, Predictor};

/// Up to 300 items: a scratch buffer of 300 x (64 + 9 bits) and a header is enough for any column of them.
const MAX_ITEMS: usize = 300;

fn numeric_codec(u: &mut Unstructured) -> arbitrary::Result<Option<NumericCodec>> {
    let pick = u.int_in_range(0..=12u8)?;
    if pick == 12 {
        return Ok(None);
    }
    let predictor = [Predictor::Raw, Predictor::Delta, Predictor::DeltaOfDelta][pick as usize / 4];
    let coder = [Coder::Prefix, Coder::Packed, Coder::Constant, Coder::Plain][pick as usize % 4];
    Ok(Some(NumericCodec::new(predictor, coder)))
}

/// Items with values either random or a random walk, so that delta, packed and constant all get picked.
fn items(u: &mut Unstructured) -> arbitrary::Result<Vec<(u64, bool)>> {
    let n = u.int_in_range(0..=MAX_ITEMS)?;
    let walk = u.arbitrary::<bool>()?;
    let rate = u.int_in_range(-4i64..=4)?;
    let mut value = u.arbitrary::<u64>()?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        value = if walk {
            value.wrapping_add((rate + u.int_in_range(-3i64..=3)?) as u64)
        } else {
            u.arbitrary()?
        };
        out.push((value, u.ratio(1u8, 16)?));
    }
    Ok(out)
}

fn run(u: &mut Unstructured) -> arbitrary::Result<()> {
    let mut buf = vec![0u8; MAX_ITEMS * 16 + 64];
    match u.int_in_range(0..=6u8)? {
        0 => {
            let width = u.int_in_range(1..=64u8)?;
            let signed = u.arbitrary()?;
            let codec = numeric_codec(u)?;
            check::numeric(&mut buf, width, signed, &items(u)?, codec);
        }
        1 => {
            let n = u.int_in_range(0..=40usize)?;
            let values: Vec<u128> = (0..n).map(|_| u.arbitrary()).collect::<Result<_, _>>()?;
            check::numeric128(&mut buf, u.arbitrary()?, &values);
        }
        2 | 3 => {
            let codec = [None, Some(FloatCodec::Plain), Some(FloatCodec::Xor)][u.int_in_range(0..=2usize)?];
            let items = items(u)?;
            check::float::<f32>(&mut buf, &items, codec);
            check::float::<f64>(&mut buf, &items, codec);
        }
        4 => {
            let kind = u.int_in_range(0..=2u8)?;
            let alphabet = u.int_in_range(1..=u32::MAX)?;
            let n = u.int_in_range(0..=MAX_ITEMS)?;
            let values: Vec<u32> = (0..n)
                .map(|_| u.int_in_range(0..=alphabet.min(4)).map(|v| if v == 4 { alphabet } else { v }))
                .collect::<Result<_, _>>()?;
            check::run(&mut buf, kind, &values, u.arbitrary()?);
        }
        5 => {
            let set: Vec<Vec<u8>> = (0..u.int_in_range(1..=4usize)?)
                .map(|_| {
                    let len = u.int_in_range(0..=12usize)?;
                    u.bytes(len).map(<[u8]>::to_vec)
                })
                .collect::<Result<_, _>>()?;
            let n = u.int_in_range(0..=60usize)?;
            let values: Vec<&[u8]> = (0..n)
                .map(|_| u.choose(&set).map(Vec::as_slice))
                .collect::<Result<_, _>>()?;
            check::opaque(&mut buf, &values, u.arbitrary()?);
        }
        _ => {
            let selector = u.arbitrary()?;
            let n = u.int_in_range(0..=MAX_ITEMS)?;
            let restarts: Vec<bool> = (0..n).map(|_| u.ratio(1u8, 8)).collect::<Result<_, _>>()?;
            let rest = u.len();
            check::garbage(u.bytes(rest)?, selector, &restarts);
        }
    }
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let _ = run(&mut Unstructured::new(data));
});

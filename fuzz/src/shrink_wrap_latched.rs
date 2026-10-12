#![no_main]

//! `U1`..`U64` and `I2`..`I64` fields are read with the error kept in the reader (SW-38): for any bytes and any
//! reader state, a read gives the value, the error and the reader of the read with a `Result`, and a derived type
//! the result of reading every field with `?` (and on success the same reader).

use libfuzzer_sys::fuzz_target;
use shrink_wrap::prelude::*;
use shrink_wrap::{BufReader, Error};

fn check<'i, T: DeserializeShrinkWrap<'i> + PartialEq + core::fmt::Debug>(rd: BufReader<'i>) {
    assert!(T::LATCHES);
    let mut rd_old = rd;
    let old = rd_old.read::<T>();
    let mut rd_new = rd;
    let new = rd_new
        .read_field::<T>()
        .expect("read_field keeps its error");
    if let Ok(old) = &old {
        assert_eq!(&new, old);
    }
    assert_eq!(rd_new.latched(), old.map(|_| ()));
    assert_eq!(rd_new, rd_old);
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct Sample {
    ch: U3,
    on: bool,
    div: U12,
    off: I10,
    level: u8,
    raw: U20,
    big: U40,
    neg: I33,
}

fn sample_with_checks(rd: &mut BufReader<'_>) -> Result<Sample, Error> {
    Ok(Sample {
        ch: rd.read()?,
        on: rd.read_bool()?,
        div: rd.read()?,
        off: rd.read()?,
        level: rd.read_u8()?,
        raw: rd.read()?,
        big: rd.read()?,
        neg: rd.read()?,
    })
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq), ww_repr = u8)]
enum Ev<'i> {
    A { x: U5, y: u8, z: I7 },
    C { z: U9, list: RefVec<'i, u8>, w: U2 },
}

fn ev_with_checks<'i>(rd: &mut BufReader<'i>) -> Result<Ev<'i>, Error> {
    Ok(match rd.read_u8()? {
        0 => Ev::A {
            x: rd.read()?,
            y: rd.read_u8()?,
            z: rd.read()?,
        },
        1 => Ev::C {
            z: rd.read()?,
            list: rd.read()?,
            w: rd.read()?,
        },
        _ => return Err(Error::EnumFutureVersionOrMalformedData),
    })
}

fuzz_target!(|data: &[u8]| {
    // the first bytes pick what was read before, the rest is the buffer
    let Some((ops, buf)) = data.split_first_chunk::<4>() else {
        return;
    };
    let mut rd = BufReader::new(buf);
    for op in ops {
        match op % 8 {
            0 => drop(rd.read_bool()),
            1 => drop(rd.read_nib()),
            2 => drop(rd.read_u8()),
            3 => drop(rd.read_rev_len()),
            4 => drop(rd.read_un8(1 + (op >> 3) % 8)),
            5 => {
                if let Ok(split) = rd.split((op >> 3) as usize) {
                    rd = split;
                }
            }
            _ => {}
        }
    }
    check::<U1>(rd);
    check::<U3>(rd);
    check::<U8>(rd);
    check::<U9>(rd);
    check::<U16>(rd);
    check::<U17>(rd);
    check::<U31>(rd);
    check::<U32>(rd);
    check::<U33>(rd);
    check::<U64>(rd);
    check::<I2>(rd);
    check::<I10>(rd);
    check::<I32>(rd);
    check::<I33>(rd);
    check::<I64>(rd);

    let (mut rd_old, mut rd_new) = (rd, rd);
    let old = sample_with_checks(&mut rd_old);
    assert_eq!(Sample::des_shrink_wrap(&mut rd_new), old);
    if old.is_ok() {
        assert_eq!(rd_new, rd_old);
    }
    let (mut rd_old, mut rd_new) = (rd, rd);
    let old = ev_with_checks(&mut rd_old);
    assert_eq!(Ev::des_shrink_wrap(&mut rd_new), old);
    if old.is_ok() {
        assert_eq!(rd_new, rd_old);
    }
});

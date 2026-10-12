#![no_main]

//! `RefVec<T>` of plain numbers finds its end without decoding the elements (SW-37): for any bytes and any
//! reader state, the result and the reader afterwards are the ones of the decode pass it replaces.

use libfuzzer_sys::fuzz_target;
use shrink_wrap::prelude::*;
use shrink_wrap::{BufReader, Error};

/// The decode pass, as `RefVec<T>` does it for every other `T`.
fn pass<'i, T: DeserializeShrinkWrap<'i>>(
    rd: &mut BufReader<'i>,
) -> Result<(BufReader<'i>, usize), Error> {
    let count = rd.read_rev_len()?;
    let start = *rd;
    for _ in 0..count {
        let _item: T = rd.read()?;
    }
    Ok((start, count))
}

fn check<'i, T: DeserializeShrinkWrap<'i> + Clone + 'i>(rd: BufReader<'i>) {
    assert!(T::PLAIN_BYTES.is_some());
    let mut rd_skip = rd;
    let mut rd_pass = rd;
    let skip = rd_skip.read::<RefVec<'i, T>>().map(|list| match list {
        RefVec::Buf {
            buf,
            elements_count,
        } => (buf, elements_count),
        RefVec::Slice { .. } => unreachable!(),
    });
    assert_eq!(skip, pass::<T>(&mut rd_pass));
    assert_eq!(rd_skip, rd_pass);
    if let Ok((start, count)) = skip {
        let mut walk = start;
        for _ in 0..count {
            walk.read::<T>().expect("element of a decoded list");
        }
        assert_eq!(walk.pos(), rd_skip.pos());
    }
}

fuzz_target!(|data: &[u8]| {
    // the first bytes pick what was read before the list, the rest is the buffer
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
    check::<u8>(rd);
    check::<u16>(rd);
    check::<u32>(rd);
    check::<u64>(rd);
    check::<u128>(rd);
    check::<i8>(rd);
    check::<i16>(rd);
    check::<i32>(rd);
    check::<i64>(rd);
    check::<i128>(rd);
    check::<f32>(rd);
    check::<f64>(rd);
});

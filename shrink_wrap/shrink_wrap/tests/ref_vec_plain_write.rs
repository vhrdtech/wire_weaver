//! `RefVec<u8>` / `RefVec<i8>` write their elements as one slice: the bytes must be the ones of a write per
//! element, also after bits that leave the writer off a byte boundary, and inside `MaxLen` the way a derived struct
//! writes it.

use shrink_wrap::prelude::*;
use shrink_wrap::{BufWriter, Error};

/// A write per element, as `RefVec<T>` does for every other `T`.
fn per_element<T: SerializeShrinkWrap>(
    lead_bits: u32,
    items: &[T],
    buf: &mut [u8],
) -> Result<usize, Error> {
    let mut wr = BufWriter::new(buf);
    for _ in 0..lead_bits {
        wr.write_bool(true)?;
    }
    wr.write_rev_len(items.len())?;
    for item in items {
        wr.write(item)?;
    }
    Ok(wr.finish_and_take()?.len())
}

fn through_ref_vec<T>(lead_bits: u32, items: &[T], buf: &mut [u8]) -> Result<usize, Error>
where
    T: SerializeShrinkWrap + for<'i> DeserializeShrinkWrap<'i> + Clone,
{
    let mut wr = BufWriter::new(buf);
    for _ in 0..lead_bits {
        wr.write_bool(true)?;
    }
    RefVec::Slice { slice: items }.ser_shrink_wrap(&mut wr)?;
    Ok(wr.finish_and_take()?.len())
}

#[test]
fn same_bytes_as_a_write_per_element() {
    let bytes: Vec<u8> = (0..300u32).map(|i| (i * 37) as u8).collect();
    let signed: Vec<i8> = bytes.iter().map(|&b| b as i8).collect();
    for lead_bits in [0, 1, 3, 4, 7, 8, 9] {
        for len in [0, 1, 5, 255, 300] {
            let (mut a, mut b) = ([0u8; 512], [0u8; 512]);
            let na = per_element(lead_bits, &bytes[..len], &mut a).unwrap();
            let nb = through_ref_vec(lead_bits, &bytes[..len], &mut b).unwrap();
            assert_eq!(a[..na], b[..nb], "u8, {lead_bits} bits before, {len} bytes");
            let na = per_element(lead_bits, &signed[..len], &mut a).unwrap();
            let nb = through_ref_vec(lead_bits, &signed[..len], &mut b).unwrap();
            assert_eq!(a[..na], b[..nb], "i8, {lead_bits} bits before, {len} bytes");
        }
    }
}

#[test]
fn too_short_a_buffer_fails() {
    let bytes = [7u8; 64];
    let mut buf = [0u8; 32];
    assert!(through_ref_vec(0, &bytes, &mut buf).is_err());
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct Chunk<'i> {
    flag: bool,
    bytes: MaxLen<RefVec<'i, u8>, 512>,
    tail: u16,
}

#[test]
fn max_len_field_round_trips() {
    let data: Vec<u8> = (0..400u32).map(|i| (i * 13) as u8).collect();
    let chunk = Chunk {
        flag: true,
        bytes: MaxLen::new_unchecked(RefVec::Slice { slice: &data }),
        tail: 0xBEEF,
    };
    let mut buf = [0u8; 1024];
    let encoded = chunk.to_ww_bytes(&mut buf).unwrap().to_vec();
    let decoded = Chunk::from_ww_bytes(&encoded).unwrap();
    assert!(decoded.flag);
    assert_eq!(decoded.tail, 0xBEEF);
    assert_eq!(
        decoded
            .bytes
            .iter()
            .map(|b| b.unwrap())
            .collect::<Vec<u8>>(),
        data
    );
}

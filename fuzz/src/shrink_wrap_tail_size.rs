#![no_main]

//! `TailSize<N>` (SW-18): arbitrary values round-trip, also when read from a buffer with garbage after the
//! value; random bytes never panic the readers.

use arbitrary::Unstructured;
use libfuzzer_sys::fuzz_target;
use shrink_wrap::prelude::*;

#[derive_shrink_wrap(owned, derive(Debug, PartialEq))]
struct RecordOwned {
    ts: u32,
    size: TailSize<2>,
    name: String,
    values: Vec<f32>,
}

#[derive_shrink_wrap(owned, derive(Debug, PartialEq))]
struct OuterOwned {
    flag: bool,
    size: TailSize,
    inner: RecordOwned,
    after: Option<u8>,
}

#[derive_shrink_wrap(owned, ww_repr = u8, derive(Debug, PartialEq))]
enum MsgOwned {
    Text { size: TailSize<3>, s: String },
    Pair(u8, TailSize<1>, Vec<u8>),
    Nothing,
}

fuzz_target!(|data: &[u8]| {
    // readers on random bytes: errors are fine, panics are not
    let _ = RecordOwned::from_ww_bytes_owned(data);
    let _ = OuterOwned::from_ww_bytes_owned(data);
    let _ = MsgOwned::from_ww_bytes_owned(data);

    let mut u = Unstructured::new(data);
    let name: String = u.arbitrary().unwrap_or_default();
    let values: Vec<f32> = u.arbitrary().unwrap_or_default();
    let record = RecordOwned {
        ts: u.arbitrary().unwrap_or(0),
        size: TailSize(0),
        name: name.chars().take(200).collect(),
        values: values.into_iter().take(500).collect(),
    };
    let outer = OuterOwned {
        flag: u.arbitrary().unwrap_or(false),
        size: TailSize(0),
        inner: record,
        after: u.arbitrary().unwrap_or(None),
    };
    let msg = match u.int_in_range(0..=2).unwrap_or(2) {
        0 => MsgOwned::Text {
            size: TailSize(0),
            s: u.arbitrary::<String>().unwrap_or_default().chars().take(1000).collect(),
        },
        1 => MsgOwned::Pair(
            u.arbitrary().unwrap_or(0),
            TailSize(0),
            u.arbitrary::<Vec<u8>>().unwrap_or_default().into_iter().take(100).collect(),
        ),
        _ => MsgOwned::Nothing,
    };
    let garbage: Vec<u8> = u.arbitrary().unwrap_or_default();

    let bytes = outer.to_ww_bytes_owned().expect("serialize");
    let back = OuterOwned::from_ww_bytes_owned(&bytes).expect("deserialize");
    assert_eq!(back.flag, outer.flag);
    assert_eq!(back.after, outer.after);
    assert_eq!(back.inner.ts, outer.inner.ts);
    assert_eq!(back.inner.name, outer.inner.name);
    assert_eq!(back.inner.values.len(), outer.inner.values.len());
    assert!(
        back.inner
            .values
            .iter()
            .zip(&outer.inner.values)
            .all(|(a, b)| a.to_bits() == b.to_bits())
    );
    // the slot counts the bytes after it to the end of the value
    assert_eq!(back.size.0 as usize, bytes.len() - 1 - TailSize::<5>::LEN);
    // garbage after the value does not change the result
    let mut longer = bytes.clone();
    longer.extend_from_slice(&garbage);
    assert_eq!(OuterOwned::from_ww_bytes_owned(&longer).expect("bounded"), back);
    // a truncated value is an error, never garbage
    if bytes.len() > 1 {
        assert!(OuterOwned::from_ww_bytes_owned(&bytes[..bytes.len() - 1]).is_err());
    }

    let bytes = msg.to_ww_bytes_owned().expect("serialize");
    let back = MsgOwned::from_ww_bytes_owned(&bytes).expect("deserialize");
    match (&back, &msg) {
        (MsgOwned::Text { s: a, .. }, MsgOwned::Text { s: b, .. }) => assert_eq!(a, b),
        (MsgOwned::Pair(a, _, va), MsgOwned::Pair(b, _, vb)) => {
            assert_eq!(a, b);
            assert_eq!(va, vb);
        }
        (MsgOwned::Nothing, MsgOwned::Nothing) => {}
        other => panic!("variant changed: {other:?}"),
    }
    let mut longer = bytes;
    longer.extend_from_slice(&garbage);
    assert_eq!(MsgOwned::from_ww_bytes_owned(&longer).expect("bounded"), back);
});

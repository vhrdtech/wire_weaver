//! `Delta`, `DeltaOfDelta` and `XorFloat` (SW-31, SW-32) as fields of derived types.

use shrink_wrap::prelude::*;

#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq))]
struct Block<'i> {
    ts: u32,
    size: TailSize<2>,
    stamps: DeltaOfDelta<'i, u64>,
    counts: Delta<'i, i16>,
    temps: XorFloat<'i, f32>,
    flag: bool, // packs into the bits right after the sequence
    note: Option<Delta<'i, u8>>,
}

#[derive_shrink_wrap(borrowed, owned, ww_repr = u4, derive(Debug, PartialEq))]
enum Column<'i> {
    Percent(Delta<'i, i32>),
    Bytes { v: Delta<'i, i64> },
    Float(XorFloat<'i, f64>),
    None,
}

#[test]
fn struct_round_trip_and_garbage_after() {
    let stamps = [
        1_700_000_000u64,
        1_700_000_060,
        1_700_000_120,
        1_700_000_181,
    ];
    let counts = [5i16, 5, 6, 4, -300];
    let temps = [21.5f32, 21.5, 21.75, f32::NAN, 22.0];
    let note = [1u8, 2, 3];
    let block = Block {
        ts: 42,
        size: TailSize(0),
        stamps: DeltaOfDelta::new(&stamps),
        counts: Delta::new(&counts),
        temps: XorFloat::new(&temps),
        flag: true,
        note: Some(Delta::new(&note)),
    };
    let mut region = [0xA5u8; 128]; // a file region longer than the value
    let len = block.to_ww_bytes(&mut region).unwrap().len();
    assert!(len < 40, "{len}");

    let owned = BlockOwned::from_ww_bytes_owned(&region).unwrap(); // bounded by the TailSize slot
    assert_eq!(owned.ts, 42);
    assert_eq!(owned.stamps, DeltaOfDeltaOwned(stamps.to_vec()));
    assert_eq!(owned.counts, DeltaOwned(counts.to_vec()));
    assert_eq!(owned.temps.len(), temps.len());
    assert!(
        owned
            .temps
            .iter()
            .map(|v| v.to_bits())
            .eq(temps.iter().map(|v| v.to_bits()))
    );
    assert!(owned.flag);
    assert_eq!(owned.note, Some(DeltaOwned(note.to_vec())));
    assert_eq!(owned.to_ww_bytes_owned().unwrap(), region[..len]);

    let back = Block::from_ww_bytes(&region).unwrap();
    assert_eq!(back.stamps.iter().collect::<Vec<_>>(), stamps);
    assert_eq!(back.counts.iter().collect::<Vec<_>>(), counts);
    assert_eq!(back.temps.len(), 5);
    assert!(back.flag);
    assert_eq!(back.note.unwrap().iter().collect::<Vec<_>>(), note);
    assert_eq!(back.counts, Delta::new(&counts)); // Buf vs Slice compare by value

    // a truncated region is rejected by the slot, whatever the bit codes inside would decode to
    for cut in 7..len {
        assert!(Block::from_ww_bytes(&region[..cut]).is_err(), "cut {cut}");
    }
}

#[test]
fn enum_variants() {
    let mut buf = [0u8; 64];
    let cases = [
        ColumnOwned::Percent(DeltaOwned(vec![100, 101, 101, 99])),
        ColumnOwned::Bytes {
            v: DeltaOwned(vec![8_000_000_000, 8_000_004_096]),
        },
        ColumnOwned::Float(XorFloatOwned(vec![0.1, 0.1, 0.2])),
        ColumnOwned::None,
    ];
    for c in cases {
        let bytes = c.to_ww_bytes_owned().unwrap();
        assert_eq!(ColumnOwned::from_ww_bytes_owned(&bytes).unwrap(), c);
        let borrowed = Column::from_ww_bytes(&bytes).unwrap();
        assert_eq!(borrowed.to_ww_bytes(&mut buf).unwrap(), bytes);
    }
}

#[test]
fn random_bytes_never_panic() {
    // a cheap stand-in for the fuzz target: deterministic pseudo-random buffers through every decoder
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    for _ in 0..2000 {
        let mut buf = [0u8; 24];
        for b in &mut buf {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            *b = x as u8;
        }
        let len = (x >> 8) as usize % buf.len();
        let _ = Block::from_ww_bytes(&buf[..len]);
        let _ = BlockOwned::from_ww_bytes_owned(&buf[..len]);
        let _ = Column::from_ww_bytes(&buf[..len]);
        let _ = Delta::<u8>::from_ww_bytes(&buf[..len]);
        let _ = DeltaOfDelta::<i64>::from_ww_bytes(&buf[..len]);
        let _ = XorFloatOwned::<f64>::from_ww_bytes_owned(&buf[..len]);
    }
}

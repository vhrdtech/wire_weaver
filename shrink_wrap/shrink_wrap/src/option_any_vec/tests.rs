use super::*;
use crate::prelude::*;

const MODES: [PresenceMode; 4] = [
    PresenceMode::All,
    PresenceMode::Dense,
    PresenceMode::TwoLevel,
    PresenceMode::IndexList,
];

/// Write `u8` value `slot * 3` into every slot of `present` with `mode` (or the smallest one)
fn write_u8s<'b>(buf: &'b mut [u8], present: &[u32], mode: Option<PresenceMode>) -> &'b [u8] {
    let slots = present.iter().copied();
    let mut wr = match mode {
        Some(mode) => OptionAnyVecWriter::new_with_mode(buf, slots, mode).unwrap(),
        None => OptionAnyVecWriter::new(buf, slots).unwrap(),
    };
    for &slot in present {
        wr.write(slot, &(slot.wrapping_mul(3) as u8)).unwrap();
    }
    wr.finish_and_take().unwrap()
}

/// Read `n` `u8` slots back
fn read_u8s(bytes: &[u8], n: u32) -> Vec<Option<u8>> {
    let vec = OptionAnyVec::new(bytes).unwrap();
    let mut iter = vec.iter();
    (0..n).map(|_| iter.next::<u8>().unwrap()).collect()
}

fn expected(present: &[u32], n: u32) -> Vec<Option<u8>> {
    (0..n)
        .map(|slot| {
            present
                .contains(&slot)
                .then_some(slot.wrapping_mul(3) as u8)
        })
        .collect()
}

#[test]
fn every_mode_round_trips() {
    let sets: &[&[u32]] = &[
        &[0],
        &[5],
        &[0, 1, 2],
        &[3, 17, 30],
        &[7, 8],
        &[0, 9, 10, 63, 64, 200],
    ];
    for &set in sets {
        for mode in MODES {
            let is_all = set.iter().enumerate().all(|(i, s)| i as u32 == *s);
            if mode == PresenceMode::All && !is_all {
                let mut buf = [0u8; 64];
                assert!(matches!(
                    OptionAnyVecWriter::new_with_mode(&mut buf, set.iter().copied(), mode),
                    Err(Error::OptionAnyVecWrongSlot)
                ));
                continue;
            }
            let mut buf = [0u8; 64];
            let bytes = write_u8s(&mut buf, set, Some(mode));
            let vec = OptionAnyVec::new(bytes).unwrap();
            assert_eq!(vec.mode(), Some(mode), "{set:?}");
            assert_eq!(vec.present().collect::<Vec<_>>(), set, "{mode:?}");
            assert_eq!(read_u8s(bytes, 260), expected(set, 260), "{set:?} {mode:?}");
        }
    }
}

#[test]
fn mode_bytes() {
    // slots 3, 17, 30 of 32, u8 values 9, 51, 90
    let set = [3, 17, 30];
    let mut buf = [0u8; 64];
    // mode 1, len 31 = UNib32 0xF 0x3, 31 bits 0001 0000_0000 0000_0100 0000_0000 001, values
    assert_eq!(
        write_u8s(&mut buf, &set, Some(PresenceMode::Dense)),
        &[0x1F, 0x31, 0x00, 0x04, 0x00, 0x20, 9, 51, 90]
    );
    // mode 2, 4 groups, group bits 1011, group bytes 0001_0000 0100_0000 0000_0010, values
    assert_eq!(
        write_u8s(&mut buf, &set, Some(PresenceMode::TwoLevel)),
        &[0x24, 0xB1, 0x04, 0x00, 0x20, 9, 51, 90]
    );
    // mode 3, count 3, deltas 3, 13 = 0xD 0x1, 12 = 0xC 0x1, values
    assert_eq!(
        write_u8s(&mut buf, &set, Some(PresenceMode::IndexList)),
        &[0x33, 0x3D, 0x1C, 0x10, 9, 51, 90]
    );
    // mode 0, n = 3
    assert_eq!(
        write_u8s(&mut buf, &[0, 1, 2], Some(PresenceMode::All)),
        &[0x03, 0, 3, 6]
    );
}

#[test]
fn smallest_mode_is_chosen() {
    let cases: &[(&[u32], PresenceMode)] = &[
        (&[0, 1, 2, 3], PresenceMode::All),
        (&[0, 2, 3], PresenceMode::Dense),
        (&[3, 17, 30], PresenceMode::IndexList),
        (
            &[0, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 22],
            PresenceMode::Dense,
        ),
        // dense would be 4 + 12 + 300 bits, the index list 4 + 8 + 8 * 4 + 3 * 12
        (&[1, 2, 3, 4, 5, 6, 7, 299], PresenceMode::IndexList),
        // a few full groups far apart: two-level
        (
            &[0, 1, 2, 3, 4, 5, 6, 7, 80, 81, 82, 83, 84, 85, 86, 87],
            PresenceMode::TwoLevel,
        ),
    ];
    for &(set, mode) in cases {
        let mut buf = [0u8; 128];
        let bytes = write_u8s(&mut buf, set, None);
        let vec = OptionAnyVec::new(bytes).unwrap();
        assert_eq!(vec.mode(), Some(mode), "{set:?}");
        let stats = SlotStats::scan(set.iter().copied()).unwrap();
        for other in MODES {
            if other != PresenceMode::All || stats.is_all() {
                assert!(stats.header_bits(mode) <= stats.header_bits(other));
            }
        }
        assert_eq!(read_u8s(bytes, 320), expected(set, 320));
    }
}

#[test]
fn empty_is_no_bytes() {
    let mut buf = [0u8; 8];
    let bytes = write_u8s(&mut buf, &[], None);
    assert_eq!(bytes, &[]);
    let vec = OptionAnyVec::new(bytes).unwrap();
    assert!(vec.is_empty());
    assert_eq!(vec.present().next(), None);
    assert_eq!(read_u8s(bytes, 4), vec![None; 4]);
    // and the explicit forms of it
    for bytes in [&[0x00][..], &[0x30]] {
        let vec = OptionAnyVec::new(bytes).unwrap();
        assert!(vec.is_empty());
        assert_eq!(read_u8s(bytes, 4), vec![None; 4]);
    }
}

#[test]
fn all_slots_present() {
    let set: Vec<u32> = (0..32).collect();
    let mut buf = [0u8; 64];
    let bytes = write_u8s(&mut buf, &set, None);
    assert_eq!(bytes.len(), 2 + 32);
    assert_eq!(&bytes[..2], &[0x08, 0x40]); // mode 0, n = 32 = UNib32 0x8 0x4
    assert_eq!(read_u8s(bytes, 33), expected(&set, 33));
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct Point<'i> {
    x: u16,
    label: &'i str,
}

#[test]
fn heterogeneous_values() {
    // slots: 0 bool, 1 U4, 2 Vec<u8>, 3 String, 4 PointOwned, 5 f32, 6 Option<u8>
    let mut buf = [0u8; 128];
    let mut wr = OptionAnyVecWriter::new(&mut buf, [0u32, 1, 2, 4, 5]).unwrap();
    wr.write(0, &true).unwrap();
    wr.write(1, &U4::new(0xA).unwrap()).unwrap();
    wr.write(2, &vec![1u8, 2, 3]).unwrap();
    wr.write(
        4,
        &Point {
            x: 0x1234,
            label: "ab",
        },
    )
    .unwrap();
    wr.write(5, &1.5f32).unwrap();
    let bytes = wr.finish_and_take().unwrap();

    let vec = OptionAnyVec::new(bytes).unwrap();
    let mut iter = vec.iter();
    assert_eq!(iter.next::<bool>(), Ok(Some(true)));
    assert_eq!(iter.next::<U4>(), Ok(Some(U4::new(0xA).unwrap())));
    assert_eq!(iter.next_owned::<Vec<u8>>(), Ok(Some(vec![1, 2, 3])));
    assert_eq!(iter.next::<&str>(), Ok(None));
    assert_eq!(
        iter.next::<Point>(),
        Ok(Some(Point {
            x: 0x1234,
            label: "ab"
        }))
    );
    assert_eq!(iter.next::<f32>(), Ok(Some(1.5)));
    assert_eq!(iter.next::<Option<u8>>(), Ok(None));
}

#[test]
fn bit_values_are_packed_without_padding() {
    // 4 bool slots of 8, header and values share a byte: mode 1, len 7 (0x7), bits 1010_101, values 1,0,1,1
    let mut buf = [0u8; 8];
    let mut wr = OptionAnyVecWriter::new(&mut buf, [0u32, 2, 4, 6]).unwrap();
    for (slot, val) in [(0, true), (2, false), (4, true), (6, true)] {
        wr.write(slot, &val).unwrap();
    }
    let bytes = wr.finish_and_take().unwrap();
    assert_eq!(bytes, &[0x17, 0b1010_1011, 0b0110_0000]);
    let vec = OptionAnyVec::new(bytes).unwrap();
    let mut iter = vec.iter();
    let read: Vec<_> = (0..8).map(|_| iter.next::<bool>().unwrap()).collect();
    assert_eq!(
        read,
        [
            Some(true),
            None,
            Some(false),
            None,
            Some(true),
            None,
            Some(true),
            None
        ]
    );
}

#[test]
fn next_present_skips_absent_slots() {
    let mut buf = [0u8; 64];
    let set = [3, 17, 30];
    let bytes = write_u8s(&mut buf, &set, None);
    let vec = OptionAnyVec::new(bytes).unwrap();
    let mut iter = vec.iter();
    let mut seen = vec![];
    while let Some(slot) = iter.next_present() {
        assert_eq!(iter.slot(), slot);
        seen.push((slot, iter.next::<u8>().unwrap().unwrap()));
    }
    assert_eq!(seen, [(3, 9), (17, 51), (30, 90)]);
    assert_eq!(iter.next::<u8>(), Ok(None));
}

/// Child level: slots 1 (u16) and 3 (String)
fn write_child(wr: &mut BufWriter<'_>) {
    let mut child = OptionAnyVecBuilder::new_nested(wr, [1u32, 3]).unwrap();
    child.write(1, &0xBEEFu16, wr).unwrap();
    child.write(3, &"hi", wr).unwrap();
    child.finish(wr).unwrap();
}

#[test]
fn nested_in_place_and_from_bytes_are_the_same() {
    // parent: 0 Vec<u8>, 2 child vec, 4 u8
    let mut buf = [0u8; 64];
    let mut wr = BufWriter::new(&mut buf);
    let mut parent = OptionAnyVecBuilder::new(&mut wr, [0u32, 2, 4]).unwrap();
    parent.write(0, &vec![7u8, 8], &mut wr).unwrap();
    parent.start_slot(2).unwrap();
    write_child(&mut wr);
    parent.write(4, &0x55u8, &mut wr).unwrap();
    parent.finish(&mut wr).unwrap();
    let in_place = wr.finish_and_take().unwrap().to_vec();

    // the child written on its own, then as a value
    let mut child_buf = [0u8; 32];
    let mut child_wr = BufWriter::new(&mut child_buf);
    let mut child = OptionAnyVecBuilder::new(&mut child_wr, [1u32, 3]).unwrap();
    child.write(1, &0xBEEFu16, &mut child_wr).unwrap();
    child.write(3, &"hi", &mut child_wr).unwrap();
    child.finish(&mut child_wr).unwrap();
    let child_bytes = child_wr.finish_and_take().unwrap();
    let child = OptionAnyVec::new(child_bytes).unwrap();
    let mut buf = [0u8; 64];
    let mut wr = OptionAnyVecWriter::new(&mut buf, [0u32, 2, 4]).unwrap();
    wr.write(0, &vec![7u8, 8]).unwrap();
    wr.write(2, &child).unwrap();
    wr.write(4, &0x55u8).unwrap();
    assert_eq!(wr.finish_and_take().unwrap(), in_place);

    let vec = OptionAnyVec::new(&in_place).unwrap();
    let mut iter = vec.iter();
    assert_eq!(iter.next_owned::<Vec<u8>>(), Ok(Some(vec![7, 8])));
    assert_eq!(iter.next::<u8>(), Ok(None));
    let child = iter.next::<OptionAnyVec>().unwrap().unwrap();
    assert_eq!(iter.next::<u8>(), Ok(None));
    assert_eq!(iter.next::<u8>(), Ok(Some(0x55)));
    let mut child = child.iter();
    assert_eq!(child.next::<u16>(), Ok(None));
    assert_eq!(child.next::<u16>(), Ok(Some(0xBEEF)));
    assert_eq!(child.next::<&str>(), Ok(None));
    assert_eq!(child.next::<&str>(), Ok(Some("hi")));
    assert_eq!(child.next::<u8>(), Ok(None));
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct Update<'i> {
    seq: u8,
    props: OptionAnyVec<'i>,
    crc: u16,
}

#[test]
fn as_a_struct_field() {
    let mut props_buf = [0u8; 32];
    let props = write_u8s(&mut props_buf, &[1, 4], None);
    let props = OptionAnyVec::new(props).unwrap();
    let update = Update {
        seq: 7,
        props,
        crc: 0xABCD,
    };
    let mut buf = [0u8; 64];
    let bytes = update.to_ww_bytes(&mut buf).unwrap();
    let read = Update::from_ww_bytes(bytes).unwrap();
    assert_eq!(read, update);
    assert_eq!(read_u8s(read.props.as_bytes(), 6), expected(&[1, 4], 6));
}

#[test]
fn newer_writer_older_reader() {
    // the writer knows 40 slots, the reader 20: it reads its 20 and ignores the rest, in every mode
    let set = [0, 1, 2, 5, 19, 20, 33, 39];
    for mode in [
        PresenceMode::Dense,
        PresenceMode::TwoLevel,
        PresenceMode::IndexList,
    ] {
        let mut buf = [0u8; 64];
        let mut wr = OptionAnyVecWriter::new_with_mode(&mut buf, set, mode).unwrap();
        for slot in set {
            // values the old reader doesn't know have other types
            if slot < 20 {
                wr.write(slot, &(slot as u16)).unwrap();
            } else {
                wr.write(slot, &format!("new {slot}")).unwrap();
            }
        }
        let bytes = wr.finish_and_take().unwrap();
        let vec = OptionAnyVec::new(bytes).unwrap();
        let mut iter = vec.iter();
        for slot in 0..20 {
            let expected = set.contains(&slot).then_some(slot as u16);
            assert_eq!(iter.next::<u16>(), Ok(expected), "{mode:?}");
        }
    }
    // all present: the writer has 10, the reader 4
    let set: Vec<u32> = (0..10).collect();
    let mut buf = [0u8; 64];
    let bytes = write_u8s(&mut buf, &set, None);
    assert_eq!(read_u8s(bytes, 4), expected(&set, 4));
}

#[test]
fn older_writer_newer_reader() {
    // the writer knows 8 slots, all present, the reader 16: the 8 new ones are absent, in every mode
    let set: Vec<u32> = (0..8).collect();
    for mode in MODES {
        let mut buf = [0u8; 64];
        let bytes = write_u8s(&mut buf, &set, Some(mode));
        assert_eq!(read_u8s(bytes, 16), expected(&set, 16), "{mode:?}");
    }
    // and a sparse old set
    let set = [1, 6];
    for mode in [
        PresenceMode::Dense,
        PresenceMode::TwoLevel,
        PresenceMode::IndexList,
    ] {
        let mut buf = [0u8; 64];
        let bytes = write_u8s(&mut buf, &set, Some(mode));
        assert_eq!(read_u8s(bytes, 300), expected(&set, 300), "{mode:?}");
    }
}

#[test]
fn writer_checks_slots() {
    let mut buf = [0u8; 32];
    assert!(matches!(
        OptionAnyVecWriter::new(&mut buf, [3u32, 2]),
        Err(Error::OptionAnyVecWrongSlot)
    ));
    assert!(matches!(
        OptionAnyVecWriter::new(&mut buf, [3u32, 3]),
        Err(Error::OptionAnyVecWrongSlot)
    ));
    assert!(matches!(
        OptionAnyVecWriter::new(&mut buf, [u32::MAX]),
        Err(Error::OptionAnyVecWrongSlot)
    ));
    let mut wr = OptionAnyVecWriter::new(&mut buf, [1u32, 3]).unwrap();
    assert_eq!(wr.write(3, &0u8), Err(Error::OptionAnyVecWrongSlot));
    let mut wr = OptionAnyVecWriter::new(&mut buf, [1u32, 3]).unwrap();
    wr.write(1, &0u8).unwrap();
    assert_eq!(wr.next_slot(), Some(3));
    assert_eq!(wr.finish_and_take(), Err(Error::OptionAnyVecWrongSlot));
}

#[test]
fn set_bits() {
    let words = [0b1001_0000_0000_0000_0000_0000_0000_0001u32, 0, 0b110];
    assert_eq!(
        SetBits::new(&words).collect::<Vec<_>>(),
        [0, 28, 31, 65, 66]
    );
    assert_eq!(SetBits::new(&[]).next(), None);
    let mut buf = [0u8; 32];
    let wr = OptionAnyVecWriter::new(&mut buf, SetBits::new(&[0b101])).unwrap();
    assert_eq!(wr.next_slot(), Some(0));
}

#[test]
fn ids_near_the_top() {
    let set = [u32::MAX - 2, u32::MAX - 1];
    let mut buf = [0u8; 64];
    let bytes = write_u8s(&mut buf, &set, None);
    let vec = OptionAnyVec::new(bytes).unwrap();
    assert_eq!(vec.mode(), Some(PresenceMode::IndexList));
    assert_eq!(vec.present().collect::<Vec<_>>(), set);
}

#[test]
fn malformed_headers_are_errors() {
    let cases: &[(&[u8], Error)] = &[
        // reserved modes
        (&[0x40], Error::EnumFutureVersionOrMalformedData),
        (&[0xF0], Error::EnumFutureVersionOrMalformedData),
        // dense: len 63 bits with 1 byte left
        (&[0x1F, 0x70], Error::MalformedOptionAnyVec),
        // dense: len u32::MAX
        (
            &[0x1F, 0xFF, 0xFF, 0xFF, 0xFF, 0x30],
            Error::MalformedOptionAnyVec,
        ),
        // two-level: 15 groups, 1 byte of group bits
        (&[0x2F, 0x1F, 0xFF], Error::MalformedOptionAnyVec),
        // two-level: 1 group, non-empty, but its byte is missing
        (&[0x21, 0x80], Error::MalformedOptionAnyVec),
        // two-level: a listed group with no slot in it
        (&[0x21, 0x80, 0x00], Error::MalformedOptionAnyVec),
        // index list: count 2^31, 2 bytes
        (
            &[
                0x3F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xF2, 0x00,
            ],
            Error::MalformedUNib32,
        ),
        (&[0x3F, 0xFF, 0xFF, 0x10], Error::MalformedOptionAnyVec),
        // index list: count 7, 1 delta
        (&[0x37, 0x10], Error::MalformedOptionAnyVec),
        // index list: ids past u32::MAX
        (
            &[
                0x32, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xF3, 0x00,
            ],
            Error::MalformedUNib32,
        ),
        // index list: ids past u32::MAX - 1: delta u32::MAX - 1, then 0
        (
            &[0x32, 0xEF, 0xFF, 0xFF, 0xFF, 0xFF, 0x30],
            Error::MalformedOptionAnyVec,
        ),
        // all: count cut short
        (&[0x08], Error::OutOfBoundsReadU4),
    ];
    for (bytes, err) in cases {
        assert_eq!(OptionAnyVec::new(bytes).err(), Some(*err), "{bytes:02x?}");
    }
}

#[test]
fn truncated_input_is_an_error_at_any_length() {
    // header + sized values cut at every byte: a header error or a value read error, never a panic
    let set = [0, 3, 9, 17, 30];
    for mode in [
        PresenceMode::Dense,
        PresenceMode::TwoLevel,
        PresenceMode::IndexList,
    ] {
        let mut buf = [0u8; 128];
        let mut wr = OptionAnyVecWriter::new_with_mode(&mut buf, set, mode).unwrap();
        for slot in set {
            wr.write(slot, &(slot as u16)).unwrap();
        }
        let bytes = wr.finish_and_take().unwrap();
        for len in 1..bytes.len() {
            let Ok(vec) = OptionAnyVec::new(&bytes[..len]) else {
                continue;
            };
            let mut iter = vec.iter();
            let failed = (0..40).any(|_| iter.next::<u16>().is_err());
            assert!(failed, "{mode:?} {len}");
        }
    }
}

#[test]
fn truncated_unsized_values_never_panic() {
    // the lengths are read from the back, so a cut one may read as garbage, but it never panics or loops
    let set = [0, 3, 9, 17, 30];
    for mode in [
        PresenceMode::Dense,
        PresenceMode::TwoLevel,
        PresenceMode::IndexList,
    ] {
        let mut buf = [0u8; 128];
        let mut wr = OptionAnyVecWriter::new_with_mode(&mut buf, set, mode).unwrap();
        for slot in set {
            wr.write(slot, &format!("s{slot}")).unwrap();
        }
        let bytes = wr.finish_and_take().unwrap();
        for len in 0..bytes.len() {
            if let Ok(vec) = OptionAnyVec::new(&bytes[..len]) {
                let mut iter = vec.iter();
                for _ in 0..40 {
                    let _ = iter.next::<&str>();
                }
            }
        }
    }
}

#[test]
fn every_byte_pattern_parses_or_fails() {
    // short inputs exhaustively: an error or a vec whose slots can be walked, never a panic
    for a in 0..=255u8 {
        for b in 0..=255u8 {
            for bytes in [&[a][..], &[a, b], &[a, b, a ^ 0x5A]] {
                if let Ok(vec) = OptionAnyVec::new(bytes) {
                    let _ = vec.present().take(1 << 12).count();
                    let mut iter = vec.iter();
                    for _ in 0..64 {
                        let _ = iter.next::<u8>();
                    }
                }
            }
        }
    }
}

#[test]
fn read_slot_skips_absent_gaps() {
    let mut buf = [0u8; 64];
    let bytes = write_u8s(&mut buf, &[1, 4, 9], None);
    let vec = OptionAnyVec::new(bytes).unwrap();
    // a reader that knows slots 1, 4, 7 and 9: 7 is absent, the rest present
    let mut iter = vec.iter();
    assert_eq!(iter.read_slot::<u8>(1), Ok(Some(3)));
    assert_eq!(iter.read_slot::<u8>(4), Ok(Some(12)));
    assert_eq!(iter.read_slot::<u8>(7), Ok(None));
    assert_eq!(iter.read_slot::<u8>(9), Ok(Some(27)));
    assert_eq!(iter.read_slot::<u8>(20), Ok(None));
    // back to an earlier slot
    assert_eq!(iter.read_slot::<u8>(3), Err(Error::OptionAnyVecWrongSlot));
    // a reader that knows slots 1 and 9: present slot 4 in between can't be skipped
    let mut iter = vec.iter();
    assert_eq!(iter.read_slot::<u8>(1), Ok(Some(3)));
    assert_eq!(iter.read_slot::<u8>(9), Err(Error::OptionAnyVecUnknownSlot));
    // a reader that knows slots 1 and 4 only stops there
    let mut iter = vec.iter();
    assert_eq!(iter.read_slot::<u8>(1), Ok(Some(3)));
    assert_eq!(iter.read_slot::<u8>(4), Ok(Some(12)));
}

#[cfg(feature = "std")]
#[test]
fn owned_builder_writes_the_same_bytes() {
    let present = [0u32, 2, 3, 17];
    let mut buf = [0u8; 64];
    let mut wr = BufWriter::new(&mut buf);
    let mut b = OptionAnyVecBuilder::new_nested(&mut wr, present).unwrap();
    let mut wr_owned = crate::BufWriterOwned::new();
    let mut b_owned = OptionAnyVecBuilderOwned::new_nested(&mut wr_owned, present).unwrap();
    for slot in present {
        let s = "x".repeat(slot as usize);
        b.write(slot, &s.as_str(), &mut wr).unwrap();
        b_owned.write(slot, &s, &mut wr_owned).unwrap();
    }
    b.finish(&mut wr).unwrap();
    b_owned.finish(&mut wr_owned).unwrap();
    assert_eq!(wr.finish_and_take().unwrap(), wr_owned.finish().unwrap());

    let mut wr_owned = crate::BufWriterOwned::new();
    let mut b_owned = OptionAnyVecBuilderOwned::new(&mut wr_owned, [1u32]).unwrap();
    assert_eq!(b_owned.next_slot(), Some(1));
    assert_eq!(
        b_owned.write(2, &0u8, &mut wr_owned),
        Err(Error::OptionAnyVecWrongSlot)
    );
}

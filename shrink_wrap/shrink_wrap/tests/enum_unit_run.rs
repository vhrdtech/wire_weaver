//! SW-71: an enum's longest run of unit variants decodes apart from its other arms, to save flash. The tag -> variant
//! mapping stays that of a plain `match`, also around the run: data arms, gaps, a variant compiled out, a future tag.

use shrink_wrap::Error;
use shrink_wrap::prelude::*;

/// Like `exact_units::Prefix`: a run from tag 0, data arms and a catch-all.
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy), self_describing, ww_repr = unib32, niche)]
enum NicheCatchAll {
    U0,
    U1,
    U2,
    U3,
    U4,
    Byte(u8),
    Word(u16),
    #[future_version]
    Unknown(u16),
}

/// Two runs with data in between, a gap and a variant with its tag out of declaration order: only the longer run is
/// decoded apart.
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy), self_describing, ww_repr = u8)]
enum TwoRuns {
    B4 = 14,
    A0 = 0,
    A1 = 1,
    A2 = 2,
    A3 = 3,
    Byte(u8) = 4,
    Word(u16) = 5,
    B0 = 10,
    B1 = 11,
    B2 = 12,
    B3 = 13,
    Lone = 20,
}

/// A variant compiled out splits the run: its tag is known, not a future one.
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy), self_describing, ww_repr = unib32)]
enum CfgInRun {
    A0,
    A1,
    #[cfg(any())]
    Gone,
    B0,
    B1,
    B2,
    B3,
    Byte(u8),
    Word(u16),
}

fn round_trip<T>(value: T, expected: &[u8])
where
    T: SerializeShrinkWrap + for<'i> DeserializeShrinkWrap<'i> + PartialEq + core::fmt::Debug,
{
    let mut buf = [0u8; 16];
    let bytes = value.to_ww_bytes(&mut buf).unwrap();
    assert_eq!(bytes, expected, "{value:?}");
    assert_eq!(T::from_ww_bytes(bytes).unwrap(), value);
}

#[test]
fn niche_catch_all_same_bytes() {
    use NicheCatchAll::*;
    round_trip(U0, &[0x00]);
    round_trip(U1, &[0x10]);
    round_trip(U2, &[0x20]);
    round_trip(U3, &[0x30]);
    round_trip(U4, &[0x40]);
    round_trip(Byte(0xAB), &[0x50, 0xAB]);
    round_trip(Word(0xABCD), &[0x60, 0xCD, 0xAB]);
    round_trip(Unknown(7), &[0x70]);
}

#[test]
fn niche_catch_all_refuses_a_tag_beyond_its_field() {
    // UNib32 of 0x10000
    let mut buf = [0u8; 8];
    let mut wr = BufWriter::new(&mut buf);
    wr.write_unib32(0x10000).unwrap();
    let bytes = wr.finish().unwrap();
    assert_eq!(
        NicheCatchAll::from_ww_bytes(bytes),
        Err(Error::EnumFutureVersionOrMalformedData)
    );
}

#[test]
fn every_tag_of_two_runs() {
    use TwoRuns::*;
    let known = [
        (0, A0),
        (1, A1),
        (2, A2),
        (3, A3),
        (10, B0),
        (11, B1),
        (12, B2),
        (13, B3),
        (14, B4),
        (20, Lone),
    ];
    for tag in 0..=u8::MAX {
        let decoded = TwoRuns::from_ww_bytes(&[tag, 0x12, 0x34]);
        let expected = match tag {
            4 => Ok(Byte(0x12)),
            5 => Ok(Word(0x3412)),
            _ => known
                .iter()
                .find(|(t, _)| *t == tag)
                .map(|(_, v)| *v)
                .ok_or(Error::EnumFutureVersionOrMalformedData),
        };
        assert_eq!(decoded, expected, "tag {tag}");
    }
    for (tag, value) in known {
        round_trip(value, &[tag]);
    }
}

#[test]
fn nothing_to_read_is_not_a_variant() {
    // a latched tag that failed to read is 0, which is in the run here
    assert_eq!(TwoRuns::from_ww_bytes(&[]), Err(Error::OutOfBoundsReadU8));
    assert_eq!(
        NicheCatchAll::from_ww_bytes(&[]),
        Err(Error::OutOfBoundsReadU4)
    );
}

#[test]
fn compiled_out_variant_stays_known() {
    use CfgInRun::*;
    round_trip(A0, &[0x00]);
    round_trip(A1, &[0x10]);
    round_trip(B0, &[0x30]);
    round_trip(B3, &[0x60]);
    round_trip(Byte(0xAB), &[0x70, 0xAB]);
    assert_eq!(
        CfgInRun::from_ww_bytes(&[0x20]),
        Err(Error::EnumVariantDisabled)
    );
}

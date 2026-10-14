//! Readers and writers of different versions of a type: fields appended with `#[default = ..]` (SW-15) and enum
//! variants unknown to an older reader (SW-16).

use shrink_wrap::prelude::*;

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct Name<'i> {
    name: &'i str,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct ConfigV1<'i> {
    rate: u16,
    name: Name<'i>,
}

/// `ConfigV1` with fields appended in later versions.
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct ConfigV2<'i> {
    rate: u16,
    name: Name<'i>,
    #[default = 7]
    timeout: u16,
    // a flag can land in padding bits of the old data's last byte, which read as 0: default to None
    #[default = None]
    retries: Option<u8>,
    #[default = Ok(1)]
    mode: Result<u8, u8>,
    #[default = Name { name: "none" }]
    alias: Name<'i>,
    #[default = RefVec::new_bytes(&[])]
    tags: RefVec<'i, u8>,
}

fn v1() -> ConfigV1<'static> {
    ConfigV1 {
        rate: 1000,
        name: Name { name: "abc" },
    }
}

fn v2() -> ConfigV2<'static> {
    ConfigV2 {
        rate: 1000,
        name: Name { name: "abc" },
        timeout: 20,
        retries: Some(9),
        mode: Err(5),
        alias: Name { name: "x" },
        tags: RefVec::new_bytes(&[1, 2]),
    }
}

#[test]
fn new_reader_old_data_reads_defaults() {
    let mut buf = [0u8; 64];
    let bytes = v1().to_ww_bytes(&mut buf).unwrap();
    let config = ConfigV2::from_ww_bytes(bytes).unwrap();
    assert_eq!(
        config,
        ConfigV2 {
            rate: 1000,
            name: Name { name: "abc" },
            timeout: 7,
            retries: None,
            mode: Ok(1),
            alias: Name { name: "none" },
            tags: RefVec::new_bytes(&[]),
        }
    );
}

#[test]
fn old_reader_new_data_skips_new_fields() {
    let mut buf = [0u8; 64];
    let bytes = v2().to_ww_bytes(&mut buf).unwrap();
    assert_eq!(ConfigV1::from_ww_bytes(bytes).unwrap(), v1());
    assert_eq!(ConfigV2::from_ww_bytes(bytes).unwrap(), v2());
}

/// Nested in a parent, a value of the old type is bounded by its size slot: the new fields are missing from it.
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct ParentV1<'i> {
    config: ConfigV1<'i>,
    after: u8,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct ParentV2<'i> {
    config: ConfigV2<'i>,
    after: u8,
}

#[test]
fn nested_both_ways() {
    let mut buf = [0u8; 64];
    let old = ParentV1 {
        config: v1(),
        after: 0xAA,
    };
    let bytes = old.to_ww_bytes(&mut buf).unwrap();
    let new = ParentV2::from_ww_bytes(bytes).unwrap();
    assert_eq!(new.config.timeout, 7);
    assert_eq!(new.config.alias, Name { name: "none" });
    assert_eq!(new.after, 0xAA);

    let mut buf = [0u8; 64];
    let new = ParentV2 {
        config: v2(),
        after: 0xBB,
    };
    let bytes = new.to_ww_bytes(&mut buf).unwrap();
    let old = ParentV1::from_ww_bytes(bytes).unwrap();
    assert_eq!(old.config, v1());
    assert_eq!(old.after, 0xBB);
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct Small {
    a: u8,
    #[default = 0xBEEF]
    b: u16,
}

/// A field that starts with data left is not missing: running out of data in it is an error, not its default.
#[test]
fn truncated_field_is_an_error() {
    assert_eq!(
        Small::from_ww_bytes(&[1]).unwrap(),
        Small { a: 1, b: 0xBEEF }
    );
    assert_eq!(
        Small::from_ww_bytes(&[1, 2]),
        Err(ShrinkWrapError::OutOfBoundsReadRawSlice)
    );
    assert_eq!(
        Small::from_ww_bytes(&[1, 2, 3]).unwrap(),
        Small { a: 1, b: 0x0302 }
    );
}

/// Malformed data in an evolved field is reported, not replaced with the default.
#[test]
fn malformed_field_is_an_error() {
    let mut buf = [0u8; 64];
    let bytes = v2().to_ww_bytes(&mut buf).unwrap().to_vec();
    // "x" of the alias
    let pos = bytes.iter().position(|b| *b == b'x').unwrap();
    let mut bad = bytes.clone();
    bad[pos] = 0xFF;
    assert_eq!(
        ConfigV2::from_ww_bytes(&bad),
        Err(ShrinkWrapError::MalformedUtf8)
    );
    // the old reader never looks at it
    assert_eq!(ConfigV1::from_ww_bytes(&bad).unwrap(), v1());
}

/// Evolved fields in an enum variant.
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq), ww_repr = unib32)]
enum EventV1 {
    Started { at: u32 },
    Stopped,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq), ww_repr = unib32)]
enum EventV2 {
    Started {
        at: u32,
        #[default = None]
        by: Option<u8>,
        #[default = 1]
        count: u8,
    },
    Stopped,
}

#[test]
fn enum_variant_fields() {
    let mut buf = [0u8; 16];
    let bytes = EventV1::Started { at: 5 }.to_ww_bytes(&mut buf).unwrap();
    assert_eq!(
        EventV2::from_ww_bytes(bytes).unwrap(),
        EventV2::Started {
            at: 5,
            by: None,
            count: 1
        }
    );
    let mut buf = [0u8; 16];
    let new = EventV2::Started {
        at: 5,
        by: Some(2),
        count: 3,
    };
    let bytes = new.to_ww_bytes(&mut buf).unwrap();
    assert_eq!(
        EventV1::from_ww_bytes(bytes).unwrap(),
        EventV1::Started { at: 5 }
    );
}

/// An error enum as an old host knows it: new variants read into `Unknown` (SW-16).
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy), ww_repr = unib32)]
enum IoErrorV1 {
    InUse,
    Refused,
    /// A variant from a newer version.
    #[future_version]
    Unknown(u32),
}

/// The same enum, two versions later: a unit variant and one with data were added.
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq), ww_repr = unib32)]
enum IoErrorV3<'i> {
    InUse,
    Refused,
    NoIoLevel,
    Timeout {
        after_ms: u32,
        pin: &'i str,
    },
    #[future_version]
    Unknown(u32),
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct ReplyV1 {
    result: Result<u8, IoErrorV1>,
    after: u8,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct ReplyV3<'i> {
    result: Result<u8, IoErrorV3<'i>>,
    after: u8,
}

#[test]
fn old_reader_new_variants() {
    let mut buf = [0u8; 32];
    let bytes = IoErrorV3::NoIoLevel.to_ww_bytes(&mut buf).unwrap();
    assert_eq!(
        IoErrorV1::from_ww_bytes(bytes).unwrap(),
        IoErrorV1::Unknown(2)
    );

    // a variant with data, nested: skipped with its size slot, the field after it is read
    let reply = ReplyV3 {
        result: Err(IoErrorV3::Timeout {
            after_ms: 500,
            pin: "IO4",
        }),
        after: 0xAB,
    };
    let mut buf = [0u8; 32];
    let bytes = reply.to_ww_bytes(&mut buf).unwrap();
    assert_eq!(
        ReplyV1::from_ww_bytes(bytes).unwrap(),
        ReplyV1 {
            result: Err(IoErrorV1::Unknown(3)),
            after: 0xAB
        }
    );

    // known variants are unchanged
    let mut buf = [0u8; 32];
    let bytes = ReplyV3 {
        result: Err(IoErrorV3::Refused),
        after: 1,
    }
    .to_ww_bytes(&mut buf)
    .unwrap();
    assert_eq!(
        ReplyV1::from_ww_bytes(bytes).unwrap().result,
        Err(IoErrorV1::Refused)
    );
}

#[test]
fn future_version_is_passed_on() {
    // an old host passes the unknown value on, a newer reader sees the variant
    let mut buf = [0u8; 32];
    let bytes = IoErrorV1::Unknown(2).to_ww_bytes(&mut buf).unwrap();
    assert_eq!(
        IoErrorV3::from_ww_bytes(bytes).unwrap(),
        IoErrorV3::NoIoLevel
    );
    assert_eq!(IoErrorV1::Unknown(2).discriminant(), 2);
    assert_eq!(IoErrorV1::Refused.discriminant(), 1);
    // without the fields of a variant with data
    let bytes = IoErrorV1::Unknown(3).to_ww_bytes(&mut buf).unwrap();
    assert!(IoErrorV3::from_ww_bytes(bytes).is_err());
    // a known discriminant would be read back as that variant
    assert_eq!(
        IoErrorV1::Unknown(1).to_ww_bytes(&mut buf),
        Err(ShrinkWrapError::SubtypeOutOfRange)
    );
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq), ww_repr = u8, no_future_version)]
enum ModeV1 {
    Off,
    On,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq), ww_repr = u8)]
enum ModeV2 {
    Off,
    On,
    Blink,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq), ww_repr = u8)]
enum ModeV1Tolerant {
    Off,
    On,
    #[future_version]
    Unknown(u32),
}

#[test]
fn without_future_version_unknown_is_an_error() {
    let mut buf = [0u8; 8];
    let bytes = ModeV2::Blink.to_ww_bytes(&mut buf).unwrap();
    assert_eq!(
        ModeV1::from_ww_bytes(bytes),
        Err(ShrinkWrapError::EnumFutureVersionOrMalformedData)
    );
    assert_eq!(
        ModeV1Tolerant::from_ww_bytes(bytes).unwrap(),
        ModeV1Tolerant::Unknown(2)
    );
    // a discriminant that doesn't fit is not written
    assert_eq!(
        ModeV1Tolerant::Unknown(256).to_ww_bytes(&mut buf),
        Err(ShrinkWrapError::SubtypeOutOfRange)
    );
    // nothing to read is still an error
    assert_eq!(
        ModeV1Tolerant::from_ww_bytes(&[]),
        Err(ShrinkWrapError::OutOfBoundsReadU8)
    );
}

/// `sized` and `self_describing` enums have no size slot: only new unit variants can be read past.
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq), ww_repr = u2, sized)]
enum PullV1 {
    None,
    Up,
    #[future_version]
    Unknown(u32),
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq), ww_repr = u2, sized)]
enum PullV2 {
    None,
    Up,
    Down,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct PinV1 {
    pull: PullV1,
    level: bool,
    id: u8,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq))]
struct PinV2 {
    pull: PullV2,
    level: bool,
    id: u8,
}

#[test]
fn sized_enum_new_unit_variant() {
    // the Unknown variant changes neither the size nor the bytes
    assert_eq!(
        <PullV1 as SerializeShrinkWrap>::ELEMENT_SIZE,
        ElementSize::Sized { size_bits: 2 }
    );
    let mut buf = [0u8; 8];
    let bytes = PinV2 {
        pull: PullV2::Down,
        level: true,
        id: 7,
    }
    .to_ww_bytes(&mut buf)
    .unwrap();
    assert_eq!(
        PinV1::from_ww_bytes(bytes).unwrap(),
        PinV1 {
            pull: PullV1::Unknown(2),
            level: true,
            id: 7
        }
    );
}

/// The `discriminants` argument keeps the variant.
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq), ww_repr = unib32, discriminants)]
enum Status {
    Idle,
    Busy(u8),
    #[future_version]
    Unknown(u32),
}

#[test]
fn discriminants_enum() {
    assert_eq!(StatusDiscriminants::Busy.discriminant(), 1);
    assert_eq!(StatusDiscriminants::Unknown(5).discriminant(), 5);
    assert_eq!(Status::Unknown(5).discriminant(), 5);
}

/// SW-47: an `Unsized` enum gets `Unknown(u32)` without writing it, in the borrowed and the owned type.
#[derive_shrink_wrap(owned, derive(Debug, PartialEq), ww_repr = u8)]
enum ModeAuto<'i> {
    Off,
    On,
    Named(&'i str) = 5,
}

/// SW-47: renamed with `future_version = <Name>`, here because `Unknown` is a variant of its own.
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq), ww_repr = u8, future_version = Newer)]
enum Reason {
    Unknown,
    User,
}

/// SW-47: not added when every discriminant is taken, nor to `sized` / `self_describing` enums.
#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy), ww_repr = u1)]
enum Full {
    A,
    B,
}

#[derive_shrink_wrap(borrowed, derive(Debug, PartialEq, Clone, Copy), ww_repr = u2, self_describing)]
enum Closed {
    A,
    B,
}

#[test]
fn future_version_by_default() {
    let mut buf = [0u8; 8];
    let bytes = ModeV2::Blink.to_ww_bytes(&mut buf).unwrap();
    assert_eq!(
        ModeAuto::from_ww_bytes(bytes).unwrap(),
        ModeAuto::Unknown(2)
    );
    assert_eq!(
        ModeAutoOwned::from_ww_bytes_owned(bytes).unwrap(),
        ModeAutoOwned::Unknown(2)
    );
    assert_eq!(Reason::from_ww_bytes(bytes).unwrap(), Reason::Newer(2));
    assert_eq!(Reason::Unknown.discriminant(), 0);

    // exhaustive without a third arm: no variant was added
    let name = |e: Full, c: Closed| match (e, c) {
        (Full::A, Closed::A) => "a",
        (Full::B, _) | (_, Closed::B) => "b",
    };
    assert_eq!(name(Full::A, Closed::A), "a");
}

//! Bit-sized numbers `U1`..`U64` / `I2`..`I64` as struct fields and enum variant payloads (SW-48): they take
//! exactly their bits, next to a sub-byte discriminant and to each other, without padding to a byte.

use proptest::prelude::*;
use shrink_wrap::Error;
use shrink_wrap::prelude::*;

/// A 7-bit or a 10-bit I2C address. The variants differ in size, so `self_describing`, not `sized`.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Eq, Copy, Clone), ww_repr = u1, self_describing)]
enum I2cAddress {
    Seven(U7),
    Ten(U10),
}

/// The same with byte-aligned payloads: the discriminant is padded to a byte.
#[derive_shrink_wrap(
    borrowed,
    owned,
    derive(Debug, PartialEq, Eq, Copy, Clone),
    ww_repr = u1,
    self_describing
)]
enum I2cAddressBytes {
    Seven(u8),
    Ten(u16),
}

/// Same-sized variants can be `sized`: 1 + 7 bits.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Eq, Copy, Clone), ww_repr = u1, sized)]
enum Offset {
    Up(U7),
    Signed(I7),
}

/// Named payloads and a 2-bit discriminant: 2 + 10 + 1 + 3 = 16 bits.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Eq, Copy, Clone), ww_repr = u2, sized)]
enum Reg {
    Wide { addr: U10, write: bool, len: U3 },
    Narrow { addr: U12, bank: I2 },
    Idle(U14),
}

/// A `sized` struct of sub-byte fields and a `sized` enum: 3 + 8 + 16 + 5 = 32 bits.
#[derive_shrink_wrap(borrowed, owned, derive(Debug, PartialEq, Eq, Copy, Clone), sized)]
struct Frame {
    kind: U3,
    offset: Offset,
    reg: Reg,
    tail: I5,
}

/// Evolvable type mixing sub-byte numbers with byte-aligned and variable-length data.
#[derive_shrink_wrap(owned, derive(Debug, PartialEq, Clone))]
struct MixedOwned {
    addr: I2cAddress,
    a: U1,
    b: I16,
    list: Vec<U5>,
    c: U31,
    opt: Option<I33>,
    d: u8,
    frame: Frame,
    e: U64,
    f: I64,
}

fn bytes<T: SerializeShrinkWrap>(value: &T) -> Vec<u8> {
    let mut buf = [0u8; 64];
    value.to_ww_bytes(&mut buf).unwrap().to_vec()
}

#[test]
fn i2c_address_bytes() {
    // 0 1010000
    let seven = I2cAddress::Seven(U7::new(0x50).unwrap());
    assert_eq!(bytes(&seven), [0x50]);
    // 1 1111111111 00000
    let ten = I2cAddress::Ten(U10::new(0x3ff).unwrap());
    assert_eq!(bytes(&ten), [0xff, 0xe0]);
    // 1 0001010101 00000
    let ten = I2cAddress::Ten(U10::new(0x55).unwrap());
    assert_eq!(bytes(&ten), [0x8a, 0xa0]);
    assert_eq!(I2cAddress::from_ww_bytes(&[0x8a, 0xa0]), Ok(ten));

    // 16 and 24 bits with u8 / u16
    assert_eq!(bytes(&I2cAddressBytes::Seven(0x50)), [0x00, 0x50]);
    assert_eq!(bytes(&I2cAddressBytes::Ten(0x3ff)), [0x80, 0xff, 0x03]);
    assert_eq!(
        <I2cAddress as SerializeShrinkWrap>::ELEMENT_SIZE,
        ElementSize::SelfDescribing
    );
}

#[test]
fn sized_enum_bytes_and_size() {
    assert_eq!(
        <Offset as SerializeShrinkWrap>::ELEMENT_SIZE,
        ElementSize::Sized { size_bits: 8 }
    );
    // 0 1111111
    assert_eq!(bytes(&Offset::Up(U7::max())), [0x7f]);
    // 1 1000000
    assert_eq!(bytes(&Offset::Signed(I7::min())), [0xc0]);
    assert_eq!(
        Offset::from_ww_bytes(&[0xc0]),
        Ok(Offset::Signed(I7::min()))
    );

    assert_eq!(
        <Reg as SerializeShrinkWrap>::ELEMENT_SIZE,
        ElementSize::Sized { size_bits: 16 }
    );
    let wide = Reg::Wide {
        addr: U10::new(0x2aa).unwrap(),
        write: true,
        len: U3::new(5).unwrap(),
    };
    // 00 1010101010 1 101
    assert_eq!(bytes(&wide), [0x2a, 0xad]);
    let narrow = Reg::Narrow {
        addr: U12::new(0xabc).unwrap(),
        bank: I2::minus_one(),
    };
    // 01 101010111100 11
    assert_eq!(bytes(&narrow), [0x6a, 0xf3]);
    // 10 00000000000001
    assert_eq!(bytes(&Reg::Idle(U14::one())), [0x80, 0x01]);
    assert_eq!(Reg::from_ww_bytes(&[0x6a, 0xf3]), Ok(narrow));
    // discriminant 3 is not a variant
    assert_eq!(
        Reg::from_ww_bytes(&[0xc0, 0x00]),
        Err(Error::EnumFutureVersionOrMalformedData)
    );

    assert_eq!(
        <Frame as SerializeShrinkWrap>::ELEMENT_SIZE,
        ElementSize::Sized { size_bits: 32 }
    );
    let frame = Frame {
        kind: U3::new(0b101).unwrap(),
        offset: Offset::Signed(I7::minus_one()),
        reg: wide,
        tail: I5::new(-16).unwrap(),
    };
    // 101 1 1111111 00 1010101010 1 101 10000
    assert_eq!(bytes(&frame), [0xbf, 0xe5, 0x55, 0xb0]);
    assert_eq!(Frame::from_ww_bytes(&[0xbf, 0xe5, 0x55, 0xb0]), Ok(frame));
}

#[test]
fn conversions() {
    assert_eq!(U7::try_from(127u8), Ok(U7::max()));
    assert_eq!(U7::try_from(128u8), Err(Error::SubtypeOutOfRange));
    assert_eq!(u16::from(U10::max()), 1023);
    assert_eq!(I10::try_from(-512i16), Ok(I10::min()));
    assert_eq!(I10::try_from(512i16), Err(Error::SubtypeOutOfRange));
    assert_eq!(i16::from(I16::min()), i16::MIN);
}

fn u7() -> impl Strategy<Value = U7> {
    (0u8..=127).prop_map(|x| U7::new(x).unwrap())
}

fn i2c_address() -> impl Strategy<Value = I2cAddress> {
    prop_oneof![
        u7().prop_map(I2cAddress::Seven),
        (0u16..=1023).prop_map(|x| I2cAddress::Ten(U10::new(x).unwrap())),
    ]
}

fn offset() -> impl Strategy<Value = Offset> {
    prop_oneof![
        u7().prop_map(Offset::Up),
        (-64i8..=63).prop_map(|x| Offset::Signed(I7::new(x).unwrap())),
    ]
}

fn reg() -> impl Strategy<Value = Reg> {
    prop_oneof![
        (0u16..1024, any::<bool>(), 0u8..8).prop_map(|(a, w, l)| Reg::Wide {
            addr: U10::new(a).unwrap(),
            write: w,
            len: U3::new(l).unwrap(),
        }),
        (0u16..4096, -2i8..=1).prop_map(|(a, b)| Reg::Narrow {
            addr: U12::new(a).unwrap(),
            bank: I2::new(b).unwrap(),
        }),
        (0u16..16384).prop_map(|x| Reg::Idle(U14::new(x).unwrap())),
    ]
}

fn frame() -> impl Strategy<Value = Frame> {
    (0u8..8, offset(), reg(), -16i8..=15).prop_map(|(k, offset, reg, t)| Frame {
        kind: U3::new(k).unwrap(),
        offset,
        reg,
        tail: I5::new(t).unwrap(),
    })
}

fn mixed() -> impl Strategy<Value = MixedOwned> {
    (
        i2c_address(),
        any::<bool>(),
        any::<i16>(),
        prop::collection::vec(0u8..32, 0..6),
        0u32..(1 << 31),
        prop::option::of(-(1i64 << 32)..(1i64 << 32)),
        any::<u8>(),
        frame(),
        any::<u64>(),
        any::<i64>(),
    )
        .prop_map(|(addr, a, b, list, c, opt, d, frame, e, f)| MixedOwned {
            addr,
            a: U1::new(a as u8).unwrap(),
            b: I16::new(b).unwrap(),
            list: list.into_iter().map(|x| U5::new(x).unwrap()).collect(),
            c: U31::new(c).unwrap(),
            opt: opt.map(|x| I33::new(x).unwrap()),
            d,
            frame,
            e: U64::new(e).unwrap(),
            f: I64::new(f).unwrap(),
        })
}

proptest! {
    #[test]
    fn i2c_address_round_trip(addr in i2c_address()) {
        let b = bytes(&addr);
        let bits = match addr { I2cAddress::Seven(_) => 8, I2cAddress::Ten(_) => 11 };
        prop_assert_eq!(b.len(), (bits as usize).div_ceil(8));
        prop_assert_eq!(I2cAddress::from_ww_bytes(&b), Ok(addr));
    }

    #[test]
    fn frame_round_trip(frame in frame()) {
        let b = bytes(&frame);
        prop_assert_eq!(b.len(), 4);
        prop_assert_eq!(Frame::from_ww_bytes(&b), Ok(frame));
    }

    #[test]
    fn mixed_round_trip(value in mixed()) {
        let b = value.to_ww_bytes_owned().unwrap();
        prop_assert_eq!(MixedOwned::from_ww_bytes_owned(&b), Ok(value));
    }

    /// A truncated message never panics. It is not always an error: the lengths at the back of the buffer can
    /// be read from other bits and still make sense (the format has no checksum).
    #[test]
    fn mixed_truncated(value in mixed(), cut in any::<prop::sample::Index>()) {
        let b = value.to_ww_bytes_owned().unwrap();
        let cut = cut.index(b.len());
        let _ = MixedOwned::from_ww_bytes_owned(&b[..cut]);
    }

    /// A `sized` type has no lengths: every truncation is an error.
    #[test]
    fn frame_truncated(frame in frame(), cut in 0usize..4) {
        let b = bytes(&frame);
        prop_assert!(Frame::from_ww_bytes(&b[..cut]).is_err());
    }
}

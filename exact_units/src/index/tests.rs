use crate::units::{Byte, Frame, Item, Sample};
use crate::{Fixed, Index, IndexFrom1, Range, Seq};
use core::cmp::Ordering;
use std::format;
use std::string::ToString;

#[test]
fn index_algebra() {
    let a = Index::<Sample, u32>::new(10);
    let mut b = a + Fixed::new(5);
    assert_eq!(b.get(), 15);
    assert_eq!(b - a, Fixed::new(5));
    assert_eq!(b.checked_distance(a), Some(Fixed::new(5)));
    assert_eq!(a.checked_distance(b), None);
    b -= Fixed::new(15);
    assert_eq!(b, Index::new(0));
    assert_eq!(b.checked_sub(Fixed::new(1)), None);
    assert_eq!(a.offset(), Fixed::new(10));
    assert!(b < a);
    assert_eq!(Index::<Byte, u32>::default().get(), 0);
}

#[test]
fn index_bounds() {
    type Slot = Index<Item, u8, Range<0, 7>>;
    assert!(Slot::try_new(7).is_ok());
    assert!(Slot::try_new(8).is_err());
    const THIRD: Slot = Slot::at(Fixed::<Item, crate::prefix::One, u8, Range<0, 7>>::bounded(
        2,
    ));
    assert_eq!(THIRD.checked_add(Fixed::new(5)).map(Index::get), Some(7));
    assert_eq!(THIRD.checked_add(Fixed::new(6)), None);
    assert_eq!(THIRD.unbounded(), Index::new(2));
}

#[test]
fn index_from_1() {
    type Channel = IndexFrom1<Item, u8, Range<1, 4>>;
    assert!(Channel::try_new(0).is_err());
    assert!(Channel::try_new(5).is_err());
    let ch = Channel::try_new(4).unwrap();
    assert_eq!(ch.to_index(), Index::new(3));
    assert_eq!(Channel::from_index(ch.to_index()), Some(ch));
    assert_eq!(Channel::from_index(Index::<Item, u8>::new(4)), None);
    assert_eq!(
        IndexFrom1::<Item, u8>::from_index(Index::<Item, u8>::new(255)),
        None
    );
    assert!(IndexFrom1::<Item, u8>::try_new(0).is_err());
    assert_eq!(ch.to_string(), "4");
    assert_eq!(format!("{ch:?}"), "IndexFrom1<Item>(4 u8)");
}

#[test]
fn seq_wraps_in_serial_order() {
    let a = Seq::<Frame, u16>::new(65530);
    let b = a + Fixed::<Frame, crate::prefix::One, u16>::new(10);
    assert_eq!(b.get(), 4);
    assert!(a.is_before(b) && b.is_after(a));
    assert!(!b.is_before(a));
    assert_eq!(a.distance(b).get(), 10);
    assert_eq!(b.distance(a).get(), 65526);
    assert_eq!(Seq::<Frame, u16>::new(u16::MAX).next().get(), 0);
    assert_eq!(b.wrapping_sub(5), Seq::new(65535));
    // exactly half the range apart: no order (RFC 1982)
    let c = Seq::<Frame, u16>::new(0);
    let d = Seq::<Frame, u16>::new(32768);
    assert!(!c.is_before(d) && !d.is_before(c));
    assert_eq!(c.serial_cmp(d), None);
    assert_eq!(c.serial_cmp(Seq::new(32767)), Some(Ordering::Less));
    assert_eq!(c.serial_cmp(Seq::new(32769)), Some(Ordering::Greater));
    assert_eq!(c.serial_cmp(c), Some(Ordering::Equal));
    assert_eq!(format!("{a:?}"), "Seq<Frame>(65530 u16)");
    assert_eq!(a.to_string(), "65530");
}

#[cfg(feature = "shrink_wrap")]
#[test]
fn seq_nibble_and_unib32() {
    use shrink_wrap::{Nibble, UNib32};
    let a = Seq::<Frame, Nibble>::new(Nibble::new(15).unwrap());
    let b = a.next();
    assert_eq!(b.get(), Nibble::new(0).unwrap());
    assert!(a.is_before(b));
    assert!(b.is_before(Seq::new(Nibble::new(7).unwrap())));
    assert!(!b.is_before(Seq::new(Nibble::new(8).unwrap())));
    let s = Seq::<Frame, UNib32>::new(UNib32(u32::MAX)).next();
    assert_eq!(s.get(), UNib32(0));
}

#[cfg(feature = "shrink_wrap")]
#[test]
fn same_bytes_as_the_number() {
    use shrink_wrap::Nibble;
    use shrink_wrap::prelude::*;
    let mut buf = [0u8; 8];
    let i = Index::<Sample, u32>::new(0x0102_0304);
    assert_eq!(
        i.to_ww_bytes(&mut buf).unwrap(),
        0x0102_0304u32.to_ww_bytes(&mut [0u8; 8]).unwrap()
    );
    let one = IndexFrom1::<Item, u8>::try_new(3).unwrap();
    let bytes = one.to_ww_bytes(&mut buf).unwrap().to_vec();
    assert_eq!(bytes, [3]);
    assert_eq!(IndexFrom1::<Item, u8>::from_ww_bytes(&bytes).unwrap(), one);
    assert!(IndexFrom1::<Item, u8>::from_ww_bytes(&[0]).is_err());
    type Ch = IndexFrom1<Item, u8, Range<1, 4>>;
    assert!(Ch::from_ww_bytes(&[5]).is_err());
    let s = Seq::<Frame, Nibble>::new(Nibble::new(0xA).unwrap());
    assert_eq!(
        s.to_ww_bytes(&mut buf).unwrap(),
        Nibble::new(0xA)
            .unwrap()
            .to_ww_bytes(&mut [0u8; 8])
            .unwrap()
    );
}

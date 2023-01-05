use crate::error::XpiError;
use core::fmt::{Display, Formatter};
use vhl_stdlib::serdes::traits::SerializeVlu4;
use vhl_stdlib::serdes::vlu4::vlu32::Vlu32;
use vhl_stdlib::serdes::vlu4::Vlu4Vec;
use vhl_stdlib::serdes::{nibble_buf, DeserializeVlu4, NibbleBuf, NibbleBufMut, SerDesSize};

/// Mask that allows to select many resources at a particular level. Used in combination with [Uri] to
/// select the level to which UriMask applies.
/// /a
///     /1
///     /2
///     /3
/// /b
///     /x
///     /y
///     /z
///         /u
///         /v
/// For example at level /a LevelMask::ByBitfield(0b011) selects /a/2 and /a/3
/// If the same mask were applied at level /b then /b/y and /b/z would be selected.
#[derive(Copy, Clone, Debug)]
pub enum UriMask<I> {
    /// Allows to choose any subgroup of up to 128 resources
    /// Resource serial are mapped as Little Endian, so that adding resources to the end do not change previously used masks.
    ByBitfield8(u8),
    ByBitfield16(u16),
    ByBitfield32(u32),
    // ByBitfield64(u64),
    // ByBitfield128(u128),
    /// Allows to choose one or more resource by their indices
    ByIndices(I),
    /// Select all resources, either resource count must to be known, or endless iterator must be
    /// stopped later
    All(Vlu32),
}

impl<I: IntoIterator<Item=u32> + Clone> UriMask<I> {
    pub fn iter(&self) -> UriMaskIter<I::IntoIter> {
        match self {
            UriMask::ByBitfield8(mask) => UriMaskIter::ByBitfield8 { mask: *mask, pos: 0 },
            UriMask::ByBitfield16(mask) => UriMaskIter::ByBitfield16 { mask: *mask, pos: 0 },
            UriMask::ByBitfield32(mask) => UriMaskIter::ByBitfield32 { mask: *mask, pos: 0 },
            UriMask::ByIndices(iter) => UriMaskIter::ByIndices { iter: iter.clone().into_iter() },
            UriMask::All(count) => UriMaskIter::All {
                count: count.0,
                pos: 0,
            },
        }
    }
}

impl<'i> DeserializeVlu4<'i> for UriMask<Vlu4Vec<'i, u32>> {
    type Error = XpiError;

    fn des_vlu4<'di>(rdr: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        let mask_kind = rdr.get_nibble()?;
        match mask_kind {
            0 => Ok(UriMask::ByBitfield8(rdr.get_u8()?)),
            1 => Ok(UriMask::ByBitfield16(rdr.get_u16_be()?)),
            2 => Ok(UriMask::ByBitfield32(rdr.get_u32_be()?)),
            3 => {
                // u64
                Err(XpiError::UriMaskUnsupportedType)
            }
            4 => {
                // u128
                Err(XpiError::UriMaskUnsupportedType)
            }
            5 => {
                let arr: Vlu4Vec<u32> = rdr.des_vlu4()?;
                Ok(UriMask::ByIndices(arr))
            },
            6 => {
                let amount = rdr.get_vlu4_u32()?;
                Ok(UriMask::All(Vlu32(amount)))
            }
            7 => Err(XpiError::ReservedDiscard),
            _ => {
                // unreachable!()
                Err(XpiError::Internal)
            }
        }
    }
}

impl<'i> SerializeVlu4 for UriMask<Vlu4Vec<'i, u32>> {
    type Error = nibble_buf::Error;

    fn ser_vlu4(&self, wgr: &mut NibbleBufMut) -> Result<(), Self::Error> {
        match self {
            UriMask::ByBitfield8(b) => {
                wgr.put_nibble(0)?;
                wgr.put_u8(*b)?;
            }
            UriMask::ByBitfield16(b) => {
                wgr.put_nibble(1)?;
                wgr.put_u16_be(*b)?;
            }
            UriMask::ByBitfield32(b) => {
                wgr.put_nibble(2)?;
                wgr.put_u32_be(*b)?;
            }
            UriMask::ByIndices(arr) => {
                // let arr_iter = &mut arr.clone();
                // wgr.unfold_as_vec(|| arr_iter.next())?;
                wgr.put(arr)?;
            }
            UriMask::All(max) => {
                wgr.put_nibble(6)?;
                wgr.put(max)?;
            }
        }
        Ok(())
    }

    fn len_nibbles(&self) -> SerDesSize {
        match self {
            UriMask::ByBitfield8(_) => SerDesSize::Sized(3),
            UriMask::ByBitfield16(_) => SerDesSize::Sized(5),
            UriMask::ByBitfield32(_) => SerDesSize::Sized(9),
            UriMask::ByIndices(_arr) => SerDesSize::Unsized, // TODO: Vlu4Vec::serdes_size() here
            UriMask::All(max) => max.len_nibbles(),
        }
    }
}

pub enum UriMaskIter<I> {
    ByBitfield8 { mask: u8, pos: u32 },
    ByBitfield16 { mask: u16, pos: u32 },
    ByBitfield32 { mask: u32, pos: u32 },
    ByIndices { iter: I },
    All { count: u32, pos: u32 },
}

macro_rules! next_one_bit {
    ($mask:ident, $pos:ident, $bit_count:literal) => {
        if *$pos < $bit_count {
            loop {
                *$pos += 1;
                let selected = *$mask & (1 << ($bit_count - 1)) != 0;
                *$mask <<= 1;
                if selected {
                    return Some(*$pos - 1);
                } else {
                    if *$pos < $bit_count {
                        continue;
                    } else {
                        break;
                    }
                }
            }
            None
        } else {
            None
        }
    };
}

impl<I: IntoIterator<Item=u32> + Clone> IntoIterator for UriMask<I>
    where <I as IntoIterator>::IntoIter: Clone,
{
    type Item = u32;
    type IntoIter = UriMaskIter<I::IntoIter>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<I: Iterator<Item=u32> + Clone> Iterator for UriMaskIter<I>
    where <I as IntoIterator>::IntoIter: Clone,
{
    type Item = u32;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            UriMaskIter::ByBitfield8 { mask, pos } => next_one_bit!(mask, pos, 8),
            UriMaskIter::ByBitfield16 { mask, pos } => next_one_bit!(mask, pos, 16),
            UriMaskIter::ByBitfield32 { mask, pos } => next_one_bit!(mask, pos, 32),
            UriMaskIter::ByIndices { iter } => iter.next(),
            UriMaskIter::All { count, pos } => {
                if *pos < *count {
                    *pos += 1;
                    Some(*pos - 1)
                } else {
                    None
                }
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        match self {
            UriMaskIter::ByBitfield8 { mask, .. } => count_ones(*mask as u32),
            UriMaskIter::ByBitfield16 { mask, .. } => count_ones(*mask as u32),
            UriMaskIter::ByBitfield32 { mask, .. } => count_ones(*mask),
            UriMaskIter::ByIndices { iter } => iter.size_hint(),
            UriMaskIter::All { count, .. } => (*count as usize, Some(*count as usize)),
        }
    }
}

fn count_ones(mut num: u32) -> (usize, Option<usize>) {
    let mut count = 0;
    for _ in 0..32 {
        if num & 0b1 != 0 {
            count += 1;
        }
        num >>= 1;
    }
    (count, Some(count))
}

impl<I: IntoIterator<Item=u32> + Clone> Display for UriMask<I>
    where <I as IntoIterator>::IntoIter: Clone,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        if f.alternate() {
            write!(f, "{{")?;
        } else {
            write!(f, "UriMask(")?;
        }
        let iter = self.iter();
        let len = iter.size_hint().0;
        for (i, id) in iter.enumerate() {
            write!(f, "{}", id)?;
            if i < len - 1 {
                write!(f, ", ")?;
            }
        }
        if f.alternate() {
            write!(f, "}}")
        } else {
            write!(f, ")")
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use vhl_stdlib::serdes::NibbleBuf;

    #[test]
    fn test_mask_u8() {
        let mask = UriMask::<Vlu4Vec<u32>>::ByBitfield8(0b1010_0001);
        let mut mask_iter = mask.iter();
        assert_eq!(mask_iter.size_hint(), (3, Some(3)));
        assert_eq!(mask_iter.next(), Some(0));
        assert_eq!(mask_iter.next(), Some(2));
        assert_eq!(mask_iter.next(), Some(7));
        assert_eq!(mask_iter.next(), None);
    }

    #[test]
    fn test_mask_u32() {
        let mask = UriMask::<Vlu4Vec<u32>>::ByBitfield32(0b1000_0000_0000_1000_0000_0000_0000_0001);
        let mut mask_iter = mask.iter();
        assert_eq!(mask_iter.size_hint(), (3, Some(3)));
        assert_eq!(mask_iter.next(), Some(0));
        assert_eq!(mask_iter.next(), Some(12));
        assert_eq!(mask_iter.next(), Some(31));
        assert_eq!(mask_iter.next(), None);
    }

    #[test]
    fn test_mask_array() {
        let buf = [0b0010_1111, 0b0111_0001];
        let mut buf = NibbleBuf::new_all(&buf);
        let arr: Vlu4Vec<u32> = buf.des_vlu4().unwrap();
        let mask = UriMask::<Vlu4Vec<u32>>::ByIndices(arr);
        let mut mask_iter = mask.iter();
        assert_eq!(mask_iter.size_hint(), (2, Some(2)));
        assert_eq!(mask_iter.next(), Some(63));
        assert_eq!(mask_iter.next(), Some(1));
        assert_eq!(mask_iter.next(), None);
    }

    #[test]
    fn test_mask_all() {
        let mask = UriMask::<Vlu4Vec<u32>>::All(Vlu32(4));
        let mut mask_iter = mask.iter();
        assert_eq!(mask_iter.size_hint(), (4, Some(4)));
        assert_eq!(mask_iter.next(), Some(0));
        assert_eq!(mask_iter.next(), Some(1));
        assert_eq!(mask_iter.next(), Some(2));
        assert_eq!(mask_iter.next(), Some(3));
        assert_eq!(mask_iter.next(), None);
    }
}

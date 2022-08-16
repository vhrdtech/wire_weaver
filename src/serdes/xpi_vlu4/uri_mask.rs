use core::fmt::{Display, Formatter};
use crate::serdes::{NibbleBuf, DeserializeVlu4};
use crate::serdes::vlu4::{Vlu4U32Array, Vlu4U32ArrayIter};
use crate::serdes::xpi_vlu4::error::XpiVlu4Error;

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
pub enum UriMask<'i> {
    /// Allows to choose any subgroup of up to 128 resources
    /// Resource serial are mapped as Little Endian, so that adding resources to the end do not change previously used masks.
    ByBitfield8(u8),
    ByBitfield16(u16),
    ByBitfield32(u32),
    // ByBitfield64(u64),
    // ByBitfield128(u128),
    /// Allows to choose one or more resource by their indices
    ByIndices(Vlu4U32Array<'i>),
    /// Select all resources, either resource count must to be known, or endless iterator must be
    /// stopped later
    All(u32)
}

impl<'i> UriMask<'i> {
    pub fn iter(&self) -> UriMaskIter<'i> {
        match *self {
            UriMask::ByBitfield8(mask) => UriMaskIter::ByBitfield8 { mask, pos: 0 },
            UriMask::ByBitfield16(mask) => UriMaskIter::ByBitfield16 { mask, pos: 0 },
            UriMask::ByBitfield32(mask) => UriMaskIter::ByBitfield32 { mask, pos:0 },
            UriMask::ByIndices(iter) => UriMaskIter::ByIndices { iter: iter.iter() },
            UriMask::All(count) => UriMaskIter::All { count, pos: 0 }
        }
    }
}

impl<'i> DeserializeVlu4<'i> for UriMask<'i> {
    type Error = XpiVlu4Error;

    fn des_vlu4<'di>(rdr: &'di mut NibbleBuf<'i>) -> Result<Self, Self::Error> {
        let mask_kind = rdr.get_nibble()?;
        match mask_kind {
            0 => {
                Ok(UriMask::ByBitfield8(rdr.get_u8()?))
            },
            1 => {
                let mask = ((rdr.get_u8()? as u16) << 8) | rdr.get_u8()? as u16;
                Ok(UriMask::ByBitfield16(mask))
            },
            2 => {
                let mask = ((rdr.get_u8()? as u32) << 24) |
                    ((rdr.get_u8()? as u32) << 16) |
                    ((rdr.get_u8()? as u32) << 8) |
                    rdr.get_u8()? as u32;
                Ok(UriMask::ByBitfield32(mask))
            },
            3 => {
                // u64
                Err(XpiVlu4Error::UriMaskUnsupportedType)
            },
            4 => {
                // u128
                Err(XpiVlu4Error::UriMaskUnsupportedType)
            },
            5 => {
                Ok(UriMask::ByIndices(rdr.des_vlu4()?))
            },
            6 => {
                let amount = rdr.get_vlu4_u32()?;
                Ok(UriMask::All(amount))
            },
            7 => {
                Err(XpiVlu4Error::UriMaskReserved)
            },
            _ => {
                // unreachable!()
                Err(XpiVlu4Error::InternalError)
            }
        }
    }
}

pub enum UriMaskIter<'i> {
    ByBitfield8 { mask: u8, pos: u32 },
    ByBitfield16 { mask: u16, pos: u32 },
    ByBitfield32 { mask: u32, pos: u32 },
    ByIndices { iter: Vlu4U32ArrayIter<'i> },
    All { count: u32, pos: u32 }
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

impl<'i> Iterator for UriMaskIter<'i> {
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
}

impl<'i> Display for UriMask<'i> {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "UriMask(")?;
        let iter = self.iter();
        let len = iter.size_hint().0;
        for (i, id) in iter.enumerate() {
            write!(f, "{}", id)?;
            if i < len - 1 {
                write!(f, ", ")?;
            }
        }
        write!(f, ")")
    }
}


#[cfg(test)]
mod test {
    use crate::serdes::NibbleBuf;
    use super::*;

    #[test]
    fn test_mask_u8() {
        let mask = UriMask::ByBitfield8(0b1010_0001);
        let mut mask_iter = mask.iter();
        assert_eq!(mask_iter.next(), Some(0));
        assert_eq!(mask_iter.next(), Some(2));
        assert_eq!(mask_iter.next(), Some(7));
        assert_eq!(mask_iter.next(), None);
    }

    #[test]
    fn test_mask_u32() {
        let mask = UriMask::ByBitfield32(0b1000_0000_0000_1000_0000_0000_0000_0001);
        let mut mask_iter = mask.iter();
        assert_eq!(mask_iter.next(), Some(0));
        assert_eq!(mask_iter.next(), Some(12));
        assert_eq!(mask_iter.next(), Some(31));
        assert_eq!(mask_iter.next(), None);
    }

    #[test]
    fn test_mask_array() {
        let buf = [0b0010_1111, 0b0111_0001];
        let mut buf = NibbleBuf::new_all(&buf);
        let arr: Vlu4U32Array = buf.des_vlu4().unwrap();
        let mask = UriMask::ByIndices(arr);
        let mut mask_iter = mask.iter();
        assert_eq!(mask_iter.next(), Some(63));
        assert_eq!(mask_iter.next(), Some(1));
        assert_eq!(mask_iter.next(), None);
    }

    #[test]
    fn test_mask_all() {
        let mask = UriMask::All(4);
        let mut mask_iter = mask.iter();
        assert_eq!(mask_iter.next(), Some(0));
        assert_eq!(mask_iter.next(), Some(1));
        assert_eq!(mask_iter.next(), Some(2));
        assert_eq!(mask_iter.next(), Some(3));
        assert_eq!(mask_iter.next(), None);
    }
}
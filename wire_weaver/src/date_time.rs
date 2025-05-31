#[cfg(feature = "chrono")]
use chrono::{FixedOffset, NaiveDateTime, Utc};
use shrink_wrap::nib32::UNib32;
use shrink_wrap::prelude::*;
use wire_weaver_derive::derive_shrink_wrap;

/// ISO 8601 combined date and time with optional time zone and optional nanoseconds.
/// Year is stored as UNib32 shifted by 2025.
///
/// * For 2025 <= year <= 2032:
/// * Minimum size is 32 bits (no time zone and without nanoseconds).
/// * Size with timezone and without nanoseconds is 50 bits.
/// * Size with nanoseconds and without timezone is 63 bits.
/// * Size with timezone and nanoseconds is 81 bits.
/// * +4 bits for 2033 <= year <= 2088 and so on.
/// * +24 bits for year <2025.
#[derive_shrink_wrap]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DateTime {
    #[final_evolution]
    pub date: NaiveDate,

    #[final_evolution]
    pub time: NaiveTime,

    #[subtype(Offset, valid(-86_399..=86_399))]
    pub offset: Option<I18>,
}

/// ISO 8601 calendar date without timezone.
/// Size varies depending on the year, which is stored as UNib32 shifted by 2025. TODO: Shift year by another number? 1970?
/// Optimized for storing build times and other timestamps happening in real time.
/// * Minimal size is 13 bits (2025 <= year <= 2032).
/// * Size is 17 bits (2033 <= year <= 2088)
/// * Maximum size is 37 bits.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[derive_shrink_wrap]
pub struct NaiveDate {
    /// -262_142 <= year <= 262_141
    #[final_evolution]
    pub year: Year,

    #[subtype(Month, valid(1..=12))]
    pub month: U4,

    #[subtype(Day, valid(1..=31))]
    pub day: U5,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Year(i32);

/// ISO 8601 time without timezone.
/// Size is 18 bits without nanoseconds and 49 bits with nanoseconds.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
// #[derive_shrink_wrap]
pub struct NaiveTime {
    pub secs: U17,

    /// < 1B for sec 0..=58, <2B for sec 59
    pub frac: Option<U31>,
}

// #[subtype(valid(0..=86399))]
pub struct Secs(U17);

// impl DateTime {
//     pub fn from_ymd_hms_naive_opt(year: i32, month: u8, day: u8, hour: u8, min: u8, sec: u8, nano: u32) -> Option<Self> {
//         let year = Year::new(year)?;
//         Some(DateTime {
//             date,
//             time,
//             offset: None,
//         })
//     }
// }

impl SerializeShrinkWrap for Year {
    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), ShrinkWrapError> {
        let y = self.0 - 2025;
        let unsigned = if y < 0 {
            (y as u32) & ((1 << 19) - 1) // zero out 13 high bits, so that UNib32 takes no more than 7 nibbles
        } else {
            y as u32
        };
        wr.write(&UNib32(unsigned))
    }
}

impl<'i> DeserializeShrinkWrap<'i> for Year {
    fn des_shrink_wrap<'di>(
        rd: &'di mut BufReader<'i>,
        _element_size: ElementSize,
    ) -> Result<Self, ShrinkWrapError> {
        let unsigned: UNib32 = rd.read(ElementSize::Implied)?;
        let is_negative = unsigned.0 & (1 << 19) != 0;
        let y = if is_negative {
            const ONES: u32 = u32::MAX << 19;
            (ONES | unsigned.0) as i32
        } else {
            unsigned.0 as i32
        };
        let y = y + 2025;
        if (-262_142..=262_141).contains(&y) {
            Ok(Year(y))
        } else {
            Err(ShrinkWrapError::SubtypeOutOfRange)
        }
    }
}

impl Year {
    pub fn new(year: i32) -> Option<Year> {
        if (-262_142..=262_141).contains(&year) {
            Some(Year(year))
        } else {
            None
        }
    }

    pub fn year(&self) -> i32 {
        self.0
    }
}

impl NaiveTime {
    // pub fn from_hms_opt(hour: u8, minute: u8, second: u8, nano: u32) -> Option<Self> {
    //     Some(NaiveTime {
    //         secs,
    //         frac,
    //     })
    // }
}

impl SerializeShrinkWrap for NaiveTime {
    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), ShrinkWrapError> {
        wr.write(&self.secs)?;
        wr.write_bool(self.frac.is_some())?;
        if let Some(v) = &self.frac {
            wr.write(v)?;
        }
        Ok(())
    }
}

impl<'i> DeserializeShrinkWrap<'i> for NaiveTime {
    fn des_shrink_wrap<'di>(
        rd: &'di mut BufReader<'i>,
        _element_size: ElementSize,
    ) -> Result<Self, ShrinkWrapError> {
        let secs: U17 = rd.read(ElementSize::Implied)?;
        let _frac_flag = rd.read_bool()?;
        let frac = if _frac_flag {
            let frac: U31 = rd.read(ElementSize::Implied)?;
            if frac.value() >= 1_000_000_000 && secs.value() % 60 != 59 {
                return Err(ShrinkWrapError::SubtypeOutOfRange);
            }
            Some(frac)
        } else {
            None
        };
        Ok(NaiveTime { secs, frac })
    }
}

#[cfg(feature = "chrono")]
impl From<NaiveDateTime> for DateTime {
    fn from(value: NaiveDateTime) -> Self {
        todo!()
    }
}

#[cfg(feature = "chrono")]
impl Into<NaiveDateTime> for DateTime {
    fn into(self) -> NaiveDateTime {
        todo!()
    }
}

#[cfg(feature = "chrono")]
impl From<chrono::DateTime<Utc>> for DateTime {
    fn from(value: chrono::DateTime<Utc>) -> Self {
        todo!()
    }
}

#[cfg(feature = "chrono")]
impl TryInto<chrono::DateTime<Utc>> for DateTime {
    type Error = ();

    fn try_into(self) -> Result<chrono::DateTime<Utc>, Self::Error> {
        todo!()
    }
}

#[cfg(feature = "chrono")]
impl From<chrono::DateTime<FixedOffset>> for DateTime {
    fn from(value: chrono::DateTime<FixedOffset>) -> Self {
        todo!()
    }
}

#[cfg(feature = "chrono")]
impl TryInto<chrono::DateTime<FixedOffset>> for &DateTime {
    type Error = ();

    fn try_into(self) -> Result<chrono::DateTime<FixedOffset>, Self::Error> {
        todo!()
    }
}

#[cfg(feature = "chrono")]
impl From<chrono::NaiveDate> for NaiveDate {
    fn from(value: chrono::NaiveDate) -> Self {
        todo!()
    }
}

#[cfg(feature = "chrono")]
impl Into<chrono::NaiveDate> for NaiveDate {
    fn into(self) -> chrono::NaiveDate {
        todo!()
    }
}

#[cfg(feature = "chrono")]
impl From<chrono::NaiveTime> for NaiveTime {
    fn from(value: chrono::NaiveTime) -> Self {
        todo!()
    }
}

#[cfg(feature = "chrono")]
impl Into<chrono::NaiveTime> for NaiveTime {
    fn into(self) -> chrono::NaiveTime {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_literal::hex;

    #[test]
    fn v1_compatibility_not_broken() {
        let dt = DateTime {
            date: NaiveDate {
                year: Year::new(2025).unwrap(),
                month: U4::new(5).unwrap(),
                day: U5::new(30).unwrap(),
            },
            time: NaiveTime {
                secs: U17::new(58_800).unwrap(),
                frac: None,
            },
            offset: None,
        };
        let mut buf = [0u8; 12];
        let mut wr = BufWriter::new(&mut buf);
        dt.ser_shrink_wrap(&mut wr).unwrap();
        let bytes = wr.finish_and_take().unwrap();
        // println!("{bytes:02X?}");
        assert_eq!(bytes, hex!("05 F3 96 C0"));
        let mut rd = BufReader::new(bytes);
        let dt_des = DateTime::des_shrink_wrap(&mut rd, ElementSize::Implied).unwrap();
        assert_eq!(dt, dt_des);
    }
}

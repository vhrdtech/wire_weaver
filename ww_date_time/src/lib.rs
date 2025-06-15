#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "chrono")]
use chrono::{Datelike, FixedOffset, NaiveDateTime, Timelike, Utc};
use core::fmt::{Debug, Formatter};
use wire_weaver::prelude::*;

#[cfg(feature = "chrono")]
pub use chrono;

/// ISO 8601 combined date and time with optional time zone and optional nanoseconds.
/// Year is stored as UNib32 shifted by 2025.
///
/// * For 2025 <= year <= 2032:
/// * Minimum size is 32 bits (UTC time zone and without nanoseconds).
/// * Size with naive / fixed offset time zone and without nanoseconds is 53 bits.
/// * Size with nanoseconds and with UTC timezone is 63 bits.
/// * Size with naive / fixed offset time zone and nanoseconds is 84 bits.
/// * +4 bits for 2033 <= year <= 2088 and so on.
/// * +24 bits for year <2025.
#[derive_shrink_wrap]
#[self_describing]
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DateTime {
    pub date: NaiveDate,

    pub time: NaiveTime,

    pub timezone: Timezone,
}

/// Timezone information.
/// UTC is preferred ant takes only 1 bit.
#[derive_shrink_wrap]
#[ww_repr(u1)]
#[sized]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Timezone {
    UTC,
    Other(OtherTimezone),
}

/// Naive and fixed offset time zones, and room for adding up to 6 more without breaking compatibility.
#[derive_shrink_wrap]
#[ww_repr(u3)]
#[sized]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OtherTimezone {
    Naive,
    FixedOffset {
        // #[subtype(Offset, valid(-86_399..=86_399))]
        secs: I18,
    },
}

/// ISO 8601 calendar date without timezone.
/// Size varies depending on the year, which is stored as UNib32 shifted by 2025. TODO: Shift year by another number? 1970?
/// Optimized for storing build times and other timestamps happening in real time.
/// * Minimal size is 13 bits (2025 <= year <= 2032).
/// * Size is 17 bits (2033 <= year <= 2088)
/// * Maximum size is 37 bits.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[derive_shrink_wrap]
#[self_describing]
pub struct NaiveDate {
    /// -262_142 <= year <= 262_141
    pub year: Year,

    // #[subtype(Month, valid(1..=12))]
    pub month: U4,

    // #[subtype(Day, valid(1..=31))]
    pub day: U5,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Year(i32);

/// ISO 8601 time without timezone.
/// Size is 18 bits without nanoseconds and 49 bits with nanoseconds.
#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
// #[derive_shrink_wrap]
pub struct NaiveTime {
    pub secs: U17,

    /// < 1B for sec 0..=58, <2B for sec 59
    pub frac: Option<U31>,
}

// #[subtype(valid(0..=86399))]
// pub struct Secs(U17);

impl DateTime {
    pub fn from_ymd_hms_utc_opt(
        year: i32,
        month: u8,
        day: u8,
        hour: u8,
        min: u8,
        sec: u8,
        nano: u32,
    ) -> Option<Self> {
        Some(DateTime {
            date: NaiveDate::from_ymd_opt(year, month, day)?,
            time: NaiveTime::from_hms_opt(hour, min, sec, nano)?,
            timezone: Timezone::UTC,
        })
    }

    pub fn from_ymd_hms_naive_opt(
        year: i32,
        month: u8,
        day: u8,
        hour: u8,
        min: u8,
        sec: u8,
        nano: u32,
    ) -> Option<Self> {
        Some(DateTime {
            date: NaiveDate::from_ymd_opt(year, month, day)?,
            time: NaiveTime::from_hms_opt(hour, min, sec, nano)?,
            timezone: Timezone::Other(OtherTimezone::Naive),
        })
    }
}

impl NaiveDate {
    pub fn from_ymd_opt(year: i32, month: u8, day: u8) -> Option<Self> {
        if month == 0 || month > 12 || day == 0 || day > 31 {
            return None;
        }
        if !(-262_142..=262_141).contains(&year) {
            return None;
        }
        Some(NaiveDate {
            year: Year(year),
            month: U4::new(month).unwrap(),
            day: U5::new(day).unwrap(),
        })
    }
}

impl SerializeShrinkWrap for Year {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

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
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, ShrinkWrapError> {
        let unsigned: UNib32 = rd.read()?;
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
    pub const fn from_hms_opt(hour: u8, minute: u8, second: u8, nano: u32) -> Option<Self> {
        if hour > 23 || minute > 59 || second > 59 {
            return None;
        }
        if nano >= 1_000_000_000 && second <= 58 {
            return None;
        }
        if nano >= 2_000_000_000 {
            return None;
        }
        let secs = (hour as u32) * 3600 + (minute as u32) * 60 + second as u32;
        let frac = if nano != 0 {
            Some(U31::new(nano).unwrap())
        } else {
            None
        };
        Some(NaiveTime {
            secs: U17::new(secs).unwrap(),
            frac,
        })
    }
}

impl SerializeShrinkWrap for NaiveTime {
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

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
    const ELEMENT_SIZE: ElementSize = ElementSize::SelfDescribing;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, ShrinkWrapError> {
        let secs: U17 = rd.read()?;
        let _frac_flag = rd.read_bool()?;
        let frac = if _frac_flag {
            let frac: U31 = rd.read()?;
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

impl Debug for DateTime {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?} {:?} {:?}", self.date, self.time, self.timezone)?;
        // if let Some(offset) = self.offset {
        //     write!(f, " {}", offset.value())?;
        // }
        Ok(())
    }
}

impl Debug for NaiveDate {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{}-M{}-D{}",
            self.year.year(),
            self.month.value(),
            self.day.value()
        )
    }
}

impl Debug for NaiveTime {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let sec = self.secs.value() % 60;
        let mins = self.secs.value() / 60;
        let min = mins % 60;
        let hour = mins / 60;
        write!(f, "{hour}:{min}:{sec}")?;
        if let Some(nano) = &self.frac {
            write!(f, ".{}", nano.value())?;
        }
        Ok(())
    }
}

#[cfg(feature = "chrono")]
impl From<NaiveDateTime> for DateTime {
    fn from(value: NaiveDateTime) -> Self {
        DateTime {
            date: value.date().into(),
            time: value.time().into(),
            timezone: Timezone::Other(OtherTimezone::Naive),
        }
    }
}

#[cfg(feature = "chrono")]
impl From<DateTime> for NaiveDateTime {
    fn from(value: DateTime) -> Self {
        // TODO: apply offset if present?
        NaiveDateTime::new(value.date.into(), value.time.into())
    }
}

#[cfg(feature = "chrono")]
impl From<chrono::DateTime<Utc>> for DateTime {
    fn from(dt: chrono::DateTime<Utc>) -> Self {
        DateTime {
            date: dt.date_naive().into(),
            time: dt.time().into(),
            timezone: Timezone::UTC,
        }
    }
}

#[cfg(feature = "chrono")]
impl TryInto<chrono::DateTime<Utc>> for DateTime {
    type Error = ();

    fn try_into(self) -> Result<chrono::DateTime<Utc>, Self::Error> {
        // shift by offset if it is present?
        todo!()
    }
}

#[cfg(feature = "chrono")]
impl From<chrono::DateTime<FixedOffset>> for DateTime {
    fn from(value: chrono::DateTime<FixedOffset>) -> Self {
        let offset = value.offset().local_minus_utc();
        DateTime {
            date: value.date_naive().into(),
            time: value.time().into(),
            timezone: Timezone::Other(OtherTimezone::FixedOffset {
                secs: I18::new(offset).unwrap(),
            }),
        }
    }
}

#[cfg(feature = "chrono")]
impl From<chrono::NaiveDate> for NaiveDate {
    fn from(date: chrono::NaiveDate) -> Self {
        NaiveDate {
            year: Year(date.year()),
            month: U4::new(date.month() as u8).unwrap(),
            day: U5::new(date.day() as u8).unwrap(),
        }
    }
}

#[cfg(feature = "chrono")]
impl From<NaiveDate> for chrono::NaiveDate {
    fn from(value: NaiveDate) -> Self {
        chrono::NaiveDate::from_ymd_opt(
            value.year.year(),
            value.month.value() as u32,
            value.day.value() as u32,
        )
        .unwrap()
    }
}

#[cfg(feature = "chrono")]
impl From<chrono::NaiveTime> for NaiveTime {
    fn from(time: chrono::NaiveTime) -> Self {
        let frac = if time.nanosecond() == 0 {
            None
        } else {
            Some(U31::new(time.nanosecond()).unwrap())
        };
        NaiveTime {
            secs: U17::new(time.num_seconds_from_midnight()).unwrap(),
            frac,
        }
    }
}

#[cfg(feature = "chrono")]
impl From<NaiveTime> for chrono::NaiveTime {
    fn from(value: NaiveTime) -> Self {
        chrono::NaiveTime::from_num_seconds_from_midnight_opt(
            value.secs.value(),
            value.frac.map(|f| f.value()).unwrap_or(0),
        )
        .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_literal::hex;

    #[test]
    fn sanity_check() {
        const _: () = assert!(
            matches!(
                <DateTime as SerializeShrinkWrap>::ELEMENT_SIZE,
                ElementSize::SelfDescribing
            ),
            "DateTime must be UnsizedSelfDescribing because Option and child types"
        );
        const _: () = assert!(
            matches!(
                <NaiveDate as SerializeShrinkWrap>::ELEMENT_SIZE,
                ElementSize::SelfDescribing
            ),
            "NaiveDate must be UnsizedSelfDescribing because of UNib32"
        );
        const _: () = assert!(
            matches!(
                <NaiveTime as SerializeShrinkWrap>::ELEMENT_SIZE,
                ElementSize::SelfDescribing
            ),
            "NaiveTime must be UnsizedSelfDescribing because of Option"
        );
    }

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
            timezone: Timezone::UTC,
        };
        let mut buf = [0u8; 12];
        let mut wr = BufWriter::new(&mut buf);
        dt.ser_shrink_wrap(&mut wr).unwrap();
        let bytes = wr.finish_and_take().unwrap();
        // println!("{bytes:02X?}");
        assert_eq!(bytes, hex!("05 F3 96 C0"));
        let mut rd = BufReader::new(bytes);
        let dt_des = DateTime::des_shrink_wrap(&mut rd).unwrap();
        assert_eq!(dt, dt_des);
    }
}

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "chrono")]
use chrono::{Datelike, FixedOffset, NaiveDateTime, Timelike, Utc};
use core::fmt::{Debug, Formatter};
use shrink_wrap::prelude::*;

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
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash),
    self_describing
)]
pub struct DateTime {
    pub date: NaiveDate,

    pub time: NaiveTime,

    pub timezone: Timezone,
}

/// Timezone information.
/// UTC is preferred ant takes only 1 bit.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash),
    self_describing,
    ww_repr = u1
)]
pub enum Timezone {
    UTC,
    Other(OtherTimezone),
}

/// Naive and fixed offset time zones, and room for adding up to 6 more without breaking compatibility.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash),
    self_describing,
    ww_repr = u3
)]
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
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash),
    self_describing
)]
pub struct NaiveDate {
    /// -262_142 <= year <= 262_141
    pub year: Year,

    // #[subtype(Month, valid(1..=12))]
    pub month: U4,

    // #[subtype(Day, valid(1..=31))]
    pub day: U5,
}

/// Calendar year, `UNib32` of the year shifted by 2025, a negative shift cut to 20 bits (bit 19 is the sign), so that it
/// takes no more than 6 nibbles.
///
/// A derived struct, so that the API model describes what is on the wire: `ww introspect` and other dynamic clients
/// decode values from the model.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash),
    self_describing,
    validate = Year::validate
)]
pub struct Year {
    shifted: UNib32,
}

/// ISO 8601 time without timezone.
/// Size is 18 bits without nanoseconds and 49 bits with nanoseconds.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash),
    self_describing
)]
pub struct NaiveTime {
    pub secs: U17,

    /// < 1B for sec 0..=58, <2B for sec 59
    /// TODO: checked subtype when implemented
    pub frac: Option<U31>,
}

// #[subtype(valid(0..=86399))]
// pub struct Secs(U17);

impl DateTime {
    pub const fn from_ymd_hms_utc_opt(
        year: i32,
        month: u8,
        day: u8,
        hour: u8,
        min: u8,
        sec: u8,
        nano: u32,
    ) -> Option<Self> {
        let (Some(date), Some(time)) = (
            NaiveDate::from_ymd_opt(year, month, day),
            NaiveTime::from_hms_opt(hour, min, sec, nano),
        ) else {
            return None;
        };
        Some(DateTime {
            date,
            time,
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
    pub const fn from_ymd_opt(year: i32, month: u8, day: u8) -> Option<Self> {
        if month == 0 || month > 12 || day == 0 || day > 31 {
            return None;
        }
        if year < -262_142 || year > 262_141 {
            return None;
        }
        Some(NaiveDate {
            year: Year::new_unchecked(year),
            month: U4::new(month).unwrap(),
            day: U5::new(day).unwrap(),
        })
    }
}

impl Year {
    pub const fn new(year: i32) -> Option<Year> {
        if year < -262_142 || year > 262_141 {
            return None;
        }
        Some(Year::new_unchecked(year))
    }

    const fn new_unchecked(year: i32) -> Year {
        let y = year - 2025;
        let unsigned = if y < 0 {
            (y as u32) & ((1 << 20) - 1) // keep 20 bits, bit 19 is the sign: the high bits are zero on the wire
        } else {
            y as u32
        };
        Year {
            shifted: UNib32(unsigned),
        }
    }

    /// Year as shifted by 2025 in `shifted`, wraps (never panics) for a value that `validate` rejects.
    pub const fn year(&self) -> i32 {
        let unsigned = self.shifted.0;
        let is_negative = unsigned & (1 << 19) != 0;
        let y = if is_negative {
            const ONES: u32 = u32::MAX << 19;
            (ONES | unsigned) as i32
        } else {
            unsigned as i32
        };
        y.wrapping_add(2025)
    }

    /// Called on every `Year` read from bytes: `-262_142 <= year <= 262_141`, same as [Year::new].
    fn validate(&self) -> Result<(), ShrinkWrapError> {
        let unsigned = self.shifted.0;
        let y = if unsigned & (1 << 19) != 0 {
            ((u32::MAX << 19) | unsigned) as i32 as i64
        } else {
            unsigned as i64
        };
        if unsigned < (1 << 20) && (-262_142..=262_141).contains(&(y + 2025)) {
            Ok(())
        } else {
            Err(ShrinkWrapError::SubtypeOutOfRange)
        }
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
            year: Year::new_unchecked(date.year()),
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
    fn year_out_of_range_fails_to_decode() {
        // 2025 + 262_141 - 2025 = 262_141 is the last year, 0x7FFF_FFFF and a 21 bit value are not years
        for bad in [
            UNib32(0x7FFF_FFFF),
            UNib32(1 << 20),
            UNib32(262_142 - 2025 + 1),
        ] {
            let year = Year { shifted: bad };
            let mut buf = [0u8; 8];
            let bytes = year.to_ww_bytes(&mut buf).unwrap();
            assert!(
                matches!(
                    Year::from_ww_bytes(bytes),
                    Err(ShrinkWrapError::SubtypeOutOfRange)
                ),
                "{bad:?}"
            );
            // never panics, whatever it holds
            let _ = year.year();
        }
        for good in [-262_142, -1, 0, 2025, 2026, 262_141] {
            let year = Year::new(good).unwrap();
            let mut buf = [0u8; 8];
            let bytes = year.to_ww_bytes(&mut buf).unwrap();
            assert_eq!(Year::from_ww_bytes(bytes).unwrap().year(), good);
        }
    }

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

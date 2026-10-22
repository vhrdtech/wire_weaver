//! Device timestamps in nanoseconds and back, and the [SteerPointOwned] for a fit.

use super::Fit;
use ww_si::ww_numeric::{NumericBaseType, NumericValue};
use ww_si::{Prefix, Second, Unwrapper};
use ww_timebase::SteerPointOwned;

/// Why a device timestamp can't be used.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DeviceTimeError {
    /// A float, or a value that doesn't fit in `u64` nanoseconds.
    NotATime,
    /// The prefix or storage changed from the first `now()` reading.
    FormatChanged,
}

impl core::fmt::Display for DeviceTimeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            DeviceTimeError::NotATime => {
                write!(f, "device time is not an integer that fits in u64 ns")
            }
            DeviceTimeError::FormatChanged => {
                write!(f, "device time changed its prefix or storage")
            }
        }
    }
}

impl core::error::Error for DeviceTimeError {}

/// A device's time in nanoseconds and back, in the prefix and storage its `Clock::now()` answers with: unwraps a
/// `U32` of µs as it wraps, and writes [SteerPointOwned]s the device can read (a device without 64-bit numbers gets
/// 32-bit ones, wrapping as its `now()` does).
#[derive(Clone, Debug)]
pub struct DeviceTime {
    prefix: Prefix,
    storage: NumericBaseType,
    unwrapper: Unwrapper,
}

impl DeviceTime {
    /// From the first `now()` reading.
    pub fn new(now: &Second) -> Result<Self, DeviceTimeError> {
        if now.value.is_float() {
            return Err(DeviceTimeError::NotATime);
        }
        Ok(DeviceTime {
            prefix: now.prefix,
            storage: now.value.ty(),
            unwrapper: Unwrapper::default(),
        })
    }

    /// A `now()` reading (or any full-width device timestamp after the previous one, less than half a wrap
    /// later) in nanoseconds since the device's clock started.
    pub fn to_ns(&mut self, t: &Second) -> Result<u64, DeviceTimeError> {
        if t.prefix != self.prefix || t.value.ty() != self.storage {
            return Err(DeviceTimeError::FormatChanged);
        }
        let full = self
            .unwrapper
            .unwrap(*t)
            .map_err(|_| DeviceTimeError::NotATime)?;
        let v = full.value.to_i128().ok_or(DeviceTimeError::NotATime)?;
        scale(v, i8::from(self.prefix) + 9)
            .and_then(|ns| u64::try_from(ns).ok())
            .ok_or(DeviceTimeError::NotATime)
    }

    /// Nanoseconds as a device timestamp in its prefix (rounded down) and storage (wrapping).
    pub fn to_device(&self, ns: i128) -> Result<Second, DeviceTimeError> {
        self.units_to_device(self.units(ns)?)
    }

    /// The steer point for `fit` at device time `local` (ns): `local` rounded down to the device's prefix, and the
    /// shared time of exactly that value. The fit is over the host's monotonic clock, `utc_offset` (ns, from
    /// [HostClock::utc_offset](super::HostClock::utc_offset)) turns it into UTC; `error_us` is the host's estimate
    /// of its error ([Fit::error_us]).
    pub fn steer_point(
        &self,
        fit: &Fit,
        local: u64,
        utc_offset: i128,
    ) -> Result<SteerPointOwned, DeviceTimeError> {
        let units = self.units(local as i128)?;
        let exact = scale(units, i8::from(self.prefix) + 9).ok_or(DeviceTimeError::NotATime)?;
        let shared = fit.shared_at(exact as u64).round() as i128 + utc_offset;
        let shared_us =
            u64::try_from(shared.div_euclid(1000)).map_err(|_| DeviceTimeError::NotATime)?;
        Ok(SteerPointOwned {
            local: self.units_to_device(units)?,
            shared_us,
            rate_ppb: fit.rate_ppb(),
            error_us: fit.error_us(),
            steerer: None,
        })
    }

    /// `ns` in whole units of the device's prefix, rounded down.
    fn units(&self, ns: i128) -> Result<i128, DeviceTimeError> {
        scale(ns, -9 - i8::from(self.prefix)).ok_or(DeviceTimeError::NotATime)
    }

    fn units_to_device(&self, units: i128) -> Result<Second, DeviceTimeError> {
        let value: NumericValue = self
            .storage
            .from_i128_wrapping(units)
            .ok_or(DeviceTimeError::NotATime)?;
        Ok(Second::from_parts(self.prefix, value))
    }
}

/// `v * 10^exp`, rounded down for a negative `exp`; `None` on overflow.
fn scale(v: i128, exp: i8) -> Option<i128> {
    let p = 10i128.checked_pow(exp.unsigned_abs() as u32)?;
    if exp >= 0 {
        v.checked_mul(p)
    } else {
        Some(v.div_euclid(p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u32_micros_device_unwraps_and_steers_wrapped() {
        // a device with 32-bit µs: now() wraps every 71.6 minutes
        let first = Second::micro(u32::MAX - 1_000_000);
        let mut dt = DeviceTime::new(&first).unwrap();
        assert_eq!(
            dt.to_ns(&first).unwrap(),
            (u32::MAX as u64 - 1_000_000) * 1000
        );
        let after_wrap = Second::micro(500_000u32);
        let ns = dt.to_ns(&after_wrap).unwrap();
        assert_eq!(ns, ((1u64 << 32) + 500_000) * 1000);
        // a fit where shared = local + 1 s, at the same rate
        let fit = Fit {
            local: ns,
            shared: ns as f64 + 1e9,
            slope: 1.0,
            min_rtt: 0,
            max_rtt: 0,
            used: 4,
            residual_rms: 0.0,
            rate_sensitivity: 0.0,
            prior: false,
        };
        // UTC is the monotonic clock + 1.7e18 ns
        let utc = 1_700_000_000_000_000_000;
        let p = dt.steer_point(&fit, ns + 1_234, utc).unwrap();
        // 32-bit µs again, wrapped as the device's own clock: 1.234 µs is cut to 1 µs, shared of exactly that
        assert_eq!(p.local, Second::micro(500_001u32));
        let shared_ns = ns as i128 + 1000 + 1_000_000_000 + utc;
        assert_eq!(p.shared_us as i128, shared_ns / 1000);
        assert_eq!(p.rate_ppb, 0);
        // a different storage is refused
        assert_eq!(
            dt.to_ns(&Second::micro(5u64)),
            Err(DeviceTimeError::FormatChanged)
        );
        assert_eq!(
            DeviceTime::new(&Second::from_parts(Prefix::Micro, NumericValue::F32(1.0)))
                .unwrap_err(),
            DeviceTimeError::NotATime
        );
    }

    #[test]
    fn u64_micros_device() {
        let mut dt = DeviceTime::new(&Second::micro(5u64)).unwrap();
        assert_eq!(dt.to_ns(&Second::micro(7u64)).unwrap(), 7_000);
        let fit = Fit {
            local: 7_000,
            shared: 1e12,
            slope: 1.0 - 1e-6,
            min_rtt: 0,
            max_rtt: 0,
            used: 4,
            residual_rms: 0.0,
            rate_sensitivity: 0.0,
            prior: false,
        };
        let p = dt.steer_point(&fit, 7_000, 0).unwrap();
        assert_eq!(p.local, Second::micro(7u64));
        assert_eq!(p.shared_us, 1_000_000_000);
        assert_eq!(p.rate_ppb, -1000);
        // before the Unix epoch: refused
        assert!(dt.steer_point(&fit, 7_000, -2_000_000_000_000).is_err());
    }
}

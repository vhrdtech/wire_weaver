//! Device timestamps as the host gives them to the program: one rule for every `Second` a device sends as the time
//! of something (a stream event's head, each item of a batch, an observe update).

use tracing::warn;
use ww_si::{Second, UnwrapError, Unwrapper};
use ww_stream::HeadExt;

/// The device's clock as one sequence of timestamps sees it: wrapping storage (`U32` µs every 71.6 minutes, `U16` ms
/// every 65.5 s) unwrapped into a `U64` in the timestamp's prefix that counts on across wraps, whatever prefix and
/// storage the device picks and even when it changes them (see [ww_si::Unwrapper]); full-width and float times
/// stay as they are.
///
/// One per stream handle (each half of a split its own) and per observe level: timestamps of different sequences
/// may be out of order to each other. Without an anchor the first timestamp is taken as is, so after a reconnect
/// (the device may have rebooted) or a new clock epoch the count starts over from the next one.
#[derive(Clone, Debug, Default)]
pub(crate) struct DeviceTime {
    unwrapper: Unwrapper,
}

impl DeviceTime {
    /// The full device time of `t`.
    pub(crate) fn unwrap(&mut self, t: Second) -> Second {
        match self.unwrapper.unwrap(t) {
            Ok(full) => full,
            // a float time doesn't wrap
            Err(UnwrapError::NotAnInteger) => t,
            Err(e) => {
                warn!("device time {t:?}: {e}, given as it is");
                t
            }
        }
    }

    /// The device's clock may have restarted (a reconnect): the next time starts the count again.
    pub(crate) fn restart(&mut self) {
        self.unwrapper.restart();
    }

    /// Before the times of a stream event: a new clock epoch in its head restarts the count.
    pub(crate) fn on_head(&mut self, ext: Option<&HeadExt>) {
        if ext.is_some_and(|ext| ext.clock_epoch.is_some()) {
            self.restart();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ww_si::Prefix;
    use ww_si::ww_numeric::{NumericValue, UVlq32};

    fn full(t: Second) -> (Prefix, u64) {
        match t.value {
            NumericValue::U64(v) => (t.prefix, v),
            _ => panic!("{t:?}"),
        }
    }

    #[test]
    fn u32_us_wraps() {
        let mut time = DeviceTime::default();
        let mut us = u32::MAX as u64 - 100;
        for _ in 0..10 {
            let t = time.unwrap(Second::micro(us as u32));
            assert_eq!(full(t), (Prefix::Micro, us));
            us += 30;
        }
        assert!(us > 1 << 32);
    }

    #[test]
    fn u16_ms_wraps() {
        let mut time = DeviceTime::default();
        for i in 0..20u64 {
            let ms = 60_000 + i * 1_000;
            assert_eq!(
                full(time.unwrap(Second::milli(ms as u16))),
                (Prefix::Milli, ms)
            );
        }
    }

    #[test]
    fn full_width_and_floats_as_they_are() {
        let mut time = DeviceTime::default();
        let t = Second::nano(u64::MAX - 5);
        assert_eq!(time.unwrap(t), t);
        let t = Second::from_f32(1.5);
        assert_eq!(time.unwrap(t).value, NumericValue::F32(1.5));
    }

    #[test]
    fn restart_and_new_epoch() {
        let mut time = DeviceTime::default();
        time.unwrap(Second::micro(u32::MAX));
        assert_eq!(
            full(time.unwrap(Second::micro(5u32))),
            (Prefix::Micro, (1 << 32) + 5)
        );
        // no epoch: counts on
        time.on_head(Some(&HeadExt {
            codec_restart: false,
            clock_epoch: None,
        }));
        time.on_head(None);
        assert_eq!(
            full(time.unwrap(Second::micro(6u32))),
            (Prefix::Micro, (1 << 32) + 6)
        );
        // the device rebooted: its clock counts from 0 again
        time.on_head(Some(&HeadExt {
            codec_restart: false,
            clock_epoch: Some(UVlq32(2)),
        }));
        assert_eq!(full(time.unwrap(Second::micro(7u32))), (Prefix::Micro, 7));
        time.unwrap(Second::micro(u32::MAX));
        time.restart();
        assert_eq!(full(time.unwrap(Second::micro(8u32))), (Prefix::Micro, 8));
    }
}

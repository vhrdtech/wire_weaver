use shrink_wrap::prelude::U3;

/// How soon a request and its reply should go out, carried in every request (the 3-bit `priority` of
/// `ww_client_server::Request`, followed by one reserved bit) and inherited by its reply (CLT-50, SRV-42).
///
/// 8 levels, 0 the lowest and 7 the highest, as many as CAN and Cyphal carry in a frame's ID (EVO-76):
/// - `0..=3` are batched: a message shares its frame with the ones that come within the link's accumulation window
///   (the device's `frame_accumulation_time`, a few hundred µs to a few ms), which saves packets and CPU on both
///   ends. [Priority::NORMAL] (2), the default, leaves `0..=1` for background traffic and `3` for batched traffic
///   that should go first.
/// - `4..=7` are urgent ([Priority::is_urgent], [Priority::URGENT] is 4): the frame goes out as soon as the
///   message is written, on the host for the request and on the device for its reply, immediate or deferred,
///   whatever else is about to join it. The window is not waited for, so the latency is one USB frame or one
///   packet less.
///
/// The level inside each half orders requests against each other: a host or device that serves its queue by
/// priority takes the higher one first (CLT-52, SRV-44). A medium whose arbitration favours the lower number (CAN,
/// Cyphal: 0 is the most urgent) maps level `l` to `7 - l` in its frame ID.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Priority(u8);

impl Priority {
    /// The lowest level.
    pub const LOWEST: Priority = Priority(0);
    /// The default: batched within the accumulation window.
    pub const NORMAL: Priority = Priority(2);
    /// The lowest urgent level: flushed at once instead of waiting for the accumulation window.
    pub const URGENT: Priority = Priority(4);
    /// The highest level.
    pub const HIGHEST: Priority = Priority(7);

    /// `level` 0..=7, None for more.
    pub const fn new(level: u8) -> Option<Self> {
        if level <= Self::HIGHEST.0 {
            Some(Priority(level))
        } else {
            None
        }
    }

    /// `level` 0..=7, 7 for more.
    pub const fn saturating(level: u8) -> Self {
        if level <= Self::HIGHEST.0 {
            Priority(level)
        } else {
            Self::HIGHEST
        }
    }

    pub const fn level(self) -> u8 {
        self.0
    }

    /// Flushed at once instead of waiting for the accumulation window: 4 and up.
    pub const fn is_urgent(self) -> bool {
        self.0 >= Self::URGENT.0
    }

    /// As on the wire.
    pub const fn to_wire(self) -> U3 {
        U3::__from_wire(self.0 as u64)
    }

    pub const fn from_wire(level: U3) -> Self {
        Priority(level.value())
    }
}

impl Default for Priority {
    fn default() -> Self {
        Self::NORMAL
    }
}

impl From<U3> for Priority {
    fn from(level: U3) -> Self {
        Self::from_wire(level)
    }
}

impl From<Priority> for U3 {
    fn from(priority: Priority) -> Self {
        priority.to_wire()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eight_levels_upper_half_urgent() {
        assert_eq!(Priority::new(7), Some(Priority::HIGHEST));
        assert_eq!(Priority::new(8), None);
        assert_eq!(Priority::saturating(15), Priority::HIGHEST);
        let urgent: [bool; 8] =
            core::array::from_fn(|l| Priority::new(l as u8).unwrap().is_urgent());
        assert_eq!(urgent, [false, false, false, false, true, true, true, true]);
        assert!(!Priority::NORMAL.is_urgent());
        for l in 0..=7 {
            let p = Priority::new(l).unwrap();
            assert_eq!(Priority::from_wire(p.to_wire()), p);
        }
    }
}

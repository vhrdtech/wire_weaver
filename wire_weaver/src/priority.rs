use shrink_wrap::Nibble;

/// How soon a request and its reply should go out, carried in every request (the `priority` nibble of
/// `ww_client_server::Request`) and inherited by its reply (CLT-50, SRV-42).
///
/// 16 levels, 0 the lowest and 15 the highest:
/// - `0..=7` are batched: a message shares its frame with the ones that come within the link's accumulation window
///   (the device's `frame_accumulation_time`, a few hundred µs to a few ms), which saves packets and CPU on both
///   ends. [Priority::NORMAL] (4), the default, sits in the middle, leaving `0..=3` for background traffic.
/// - `8..=15` are urgent ([Priority::is_urgent], [Priority::URGENT] is 8): the frame goes out as soon as the
///   message is written, on the host for the request and on the device for its reply, immediate or deferred,
///   whatever else is about to join it. The window is not waited for, so the latency is one USB frame or one
///   packet less.
///
/// The level inside each half orders requests against each other: a host or device that serves its queue by
/// priority takes the higher one first (CLT-52, SRV-44).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Priority(u8);

impl Priority {
    /// The lowest level.
    pub const LOWEST: Priority = Priority(0);
    /// The default: batched within the accumulation window.
    pub const NORMAL: Priority = Priority(4);
    /// The lowest urgent level: flushed at once instead of waiting for the accumulation window.
    pub const URGENT: Priority = Priority(8);
    /// The highest level.
    pub const HIGHEST: Priority = Priority(15);

    /// `level` 0..=15, None for more.
    pub const fn new(level: u8) -> Option<Self> {
        if level <= 15 {
            Some(Priority(level))
        } else {
            None
        }
    }

    /// `level` 0..=15, 15 for more.
    pub const fn saturating(level: u8) -> Self {
        if level <= 15 {
            Priority(level)
        } else {
            Self::HIGHEST
        }
    }

    pub const fn level(self) -> u8 {
        self.0
    }

    /// Flushed at once instead of waiting for the accumulation window: 8 and up.
    pub const fn is_urgent(self) -> bool {
        self.0 >= Self::URGENT.0
    }

    /// As on the wire.
    pub const fn nibble(self) -> Nibble {
        Nibble::new_masked(self.0)
    }

    pub const fn from_nibble(nibble: Nibble) -> Self {
        Priority(nibble.value())
    }
}

impl Default for Priority {
    fn default() -> Self {
        Self::NORMAL
    }
}

impl From<Nibble> for Priority {
    fn from(nibble: Nibble) -> Self {
        Self::from_nibble(nibble)
    }
}

impl From<Priority> for Nibble {
    fn from(priority: Priority) -> Self {
        priority.nibble()
    }
}

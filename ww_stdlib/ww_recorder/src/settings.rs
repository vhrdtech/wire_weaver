use ww_api::prelude::*;
use ww_si::{Bytes, Millis};

/// Settings of the recorder, see [Recorder::settings].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct Settings {
    /// What a flush grows to before it is written, a multiple of 512 and the card's recording unit, at most the
    /// ring's segment payload. Default 64 KB, or the card check's [CardCheck::best_flush]. Larger writes
    /// are faster and lose more in a power cut. Takes effect at once.
    pub flush: Bytes<u32>,
    /// After how long a flush with data is written even if small (default 1000 ms): the most a power cut
    /// takes when data comes slowly. Takes effect at once.
    pub flush_interval: Millis<u32>,
    /// What to do when the ring is full. Takes effect from the next boot.
    pub full: FullPolicy,
    /// Whether tracks are written all the time or only in recordings. Takes effect from the next boot.
    pub mode: RecordingMode,
    /// How the log is split in the USB view. Takes effect at the next [Recorder::refresh_usb] or mount.
    pub usb_view: UsbView,
}

/// Behaviour of a full ring, see [Settings::full].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum FullPolicy {
    /// A logger: overwrite the oldest data.
    DropOldest,
    /// Stop writing and count what is refused ([State::Full]).
    DropNewest,
}

/// Which items are written, see [Settings::mode].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum RecordingMode {
    /// Everything is logged all the time; a recording labels a span, so the pre-trigger time is free.
    Continuous,
    /// Tracks are written only inside recordings.
    Triggered,
}

/// How the USB view splits the log, see [Settings::usb_view]. The recordings folder is always there.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct UsbView {
    /// A folder per hour of device or real time.
    pub by_time: bool,
    /// A folder per track.
    pub by_track: bool,
    /// Fixed-width CSV files next to the native ones, for tracks that have one.
    pub csv: bool,
}

/// When a track is flushed, see [Track::durability].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum Durability {
    /// With the next flush ([Settings::flush] or [Settings::flush_interval]).
    Normal,
    /// Written to the card right after each item: for events and errors. Costs a write per item.
    Immediate,
}

/// Compression of a track's batches, see [Track::compression].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    ww_repr = nib
)]
pub enum Compression {
    None,
    Lz4,
    Heatshrink,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip() {
        let s = Settings {
            flush: Bytes::new(64u32 << 10),
            flush_interval: Millis::new(1000u32),
            full: FullPolicy::DropOldest,
            mode: RecordingMode::Continuous,
            usb_view: UsbView {
                by_time: true,
                by_track: false,
                csv: true,
            },
        };
        let mut buf = [0u8; 64];
        let bytes = s.to_ww_bytes(&mut buf).unwrap();
        assert_eq!(Settings::from_ww_bytes(bytes).unwrap(), s);
    }
}

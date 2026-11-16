use ww_api::prelude::*;
use ww_si::units::{Item, Record};
use ww_si::{Bytes, Count, Index};

use crate::UnknownableRate;
use crate::error::RecorderError;
use crate::settings::{Compression, Durability};

/// Most resource ids in a [TrackInfo::path].
pub const MAX_PATH: usize = 16;

/// Stable key of a track across sessions and firmware versions that keep its path, see [TrackInfo::key].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash),
    transparent
)]
pub struct TrackKey(pub u64);

/// One track of the log, `Recorder::track[n]`.
#[ww_trait]
pub trait Track {
    /// Where the track comes from and what it carries.
    fn info() -> TrackInfo<'i>;

    /// Whether the track is written. A disabled track keeps its place in the table and its position; its items are
    /// counted as dropped. Takes effect at once; the system track (0) can't be disabled (`Unsupported`).
    #[errors(Common(Unsupported))]
    property!(rw enabled: bool, RecorderError<'i>);

    /// When the track is flushed to the card, see [Durability]. Takes effect at once.
    #[optional(track_settings)]
    #[errors(Common(Unsupported))]
    property!(rw durability: Durability, RecorderError<'i>);

    /// Compression of the track's batches, see [Compression]. Optional: a device without a codec leaves it out,
    /// and tracks are stored plain.
    #[optional]
    #[errors(Common(Unsupported))]
    property!(rw compression: Compression, RecorderError<'i>);

    /// What the track has stored and lost.
    #[optional(counting)]
    fn counters() -> TrackCounters;
}

/// A track's identity, see [Track::info]. Numbers only: the device has no resource or type names at hand, a host
/// makes them (`can[1].rx`, `ww_can_bus::Frame`) from the device's API.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug, PartialEq))]
pub struct TrackInfo<'i> {
    /// The stream's resource path in the device API as resource ids, array indices included: the form requests use
    /// (`docs/api/addressing.md`). Empty for the system track.
    pub path: MaxLen<RefVec<'i, UNib32>, MAX_PATH>,
    /// What the track's items are.
    pub item_type: ItemType,
    /// Stable key: the same track across sessions and firmware versions that keep its path. Made from
    /// numbers only (what names the API root across firmware updates, and the path).
    pub key: TrackKey,
    /// Nominal rate, unknown if the device can't tell.
    pub rate: UnknownableRate,
}

/// The type of a track's items, see [TrackInfo::item_type]. A host finds crate, version and type path in the
/// device's API from the track's path, and recognises kinds of data (a CAN frame, a defmt frame) by these.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct ItemType {
    /// The type's `ww_self` signature (`ww_self::signature`, `docs/std_library/ww_self.md`), `None` when the
    /// device doesn't know it.
    pub signature: Option<[u8; 8]>,
    /// The `ww_global` id of the type's crate (the number of its `GlobalTypeId`), when it has one.
    pub global_id: Option<UNib32>,
}

/// What a track has stored, see [Track::counters]. Since this boot unless it says otherwise.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct TrackCounters {
    /// Records and item bytes stored.
    pub records: Count<Record, UVlq64>,
    pub bytes: Bytes<UVlq64>,
    /// Where the track's stream continues (counts across boots, the stream position of the next item).
    pub next: Index<Item, UVlq64>,
    /// Up to where the track is on the card.
    pub durable: Index<Item, UVlq64>,
    /// Up to where the host released it.
    pub released: Index<Item, UVlq64>,
    /// Units refused or lost on the way: producer overflow, disabled track, full ring.
    pub dropped: Count<Item, UVlq64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn track_info_is_numbers() {
        let path = [UNib32(3), UNib32(1), UNib32(0)];
        let info = TrackInfo {
            path: MaxLen::new(RefVec::Slice { slice: &path }).unwrap(),
            item_type: ItemType {
                signature: Some([1, 2, 3, 4, 5, 6, 7, 8]),
                global_id: Some(UNib32(0)),
            },
            key: TrackKey(0x0123_4567_89ab_cdef),
            rate: UnknownableRate::new(ww_si::Fixed::try_new(2000).unwrap()),
        };
        let mut buf = [0u8; 64];
        let bytes = info.to_ww_bytes(&mut buf).unwrap();
        // the numbers and a few bytes of lengths and flags, no names
        assert!(bytes.len() <= 28, "{} bytes", bytes.len());
        assert_eq!(TrackInfo::from_ww_bytes(bytes).unwrap(), info);
    }

    #[test]
    fn counters_are_small_when_idle() {
        let mut buf = [0u8; 128];
        let z = UVlq64(0);
        let idle = TrackCounters {
            records: Count::new(z),
            bytes: Bytes::new(z),
            next: Index::new(z),
            durable: Index::new(z),
            released: Index::new(z),
            dropped: Count::new(z),
        };
        let bytes = idle.to_ww_bytes(&mut buf).unwrap();
        assert!(bytes.len() <= 8, "{} bytes", bytes.len());
    }
}

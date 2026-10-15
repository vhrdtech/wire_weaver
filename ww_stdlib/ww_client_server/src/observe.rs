//! Observing properties (API-38): the host asks the device to send a trait level's properties at chosen rates with
//! [RequestKind::Observe](crate::RequestKind::Observe), the device answers with what it granted
//! ([ObserveGranted]) and then sends one [EventKind::PropertyUpdate](crate::EventKind::PropertyUpdate) per level
//! whenever any of the observed properties is due, carrying just those.
//!
//! A level is one trait: the root API or a trait implemented with `ww_impl!`, addressed by its path. Its properties
//! are named by their resource index within the level, the same numbers as in the introspection data and in request
//! paths. Only properties declared with `+observe` can be observed.
//!
//! Periods are in microseconds (`40` is 25 kHz, `1_000_000` once a second). The device clamps every setting to
//! what it can do and to the level's byte budget, and tells the host the result.

use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::{BufWriter, Error, SerializeShrinkWrap};
use ww_si::Second;

/// How one property is observed.
///
/// Evolvable: a device reading a mode from a newer host gets `Unknown(_)` and refuses that entry
/// ([ObserveRefusal::ModeNotSupported]).
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq),
    ww_repr = u4
)]
pub enum ObserveMode {
    /// Not sent.
    Off,
    /// Sent when the value changes, at most once every `min_us`.
    OnChange { min_us: UNib32 },
    /// Sent every `every_us`, changed or not.
    Periodic { every_us: UNib32 },
    /// Sent when the value changes, at most once every `min_us`, and at least once every `max_us` even when it
    /// doesn't (a heartbeat, so the host knows the value is still current).
    OnChangeOrBeat { min_us: UNib32, max_us: UNib32 },
}

/// One property of a level and how it is (to be) observed.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq)
)]
pub struct ObserveEntry {
    /// Resource index of the property within its trait level.
    pub index: UNib32,
    pub mode: ObserveMode,
}

/// Body of [RequestKind::Observe](crate::RequestKind::Observe), sent to a trait level's path.
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, Clone))]
pub struct ObserveRequest<'i> {
    /// Settings per property, at most one entry per index.
    pub entries: RefVec<'i, ObserveEntry>,
    /// `true`: properties not listed keep their current setting. `false`: properties not listed are `Off`, so
    /// `entries: [], patch: false` stops observing the whole level.
    pub patch: bool,
    /// Bytes per second the level's updates may take on the link (events as sent, with their headers); `None`
    /// leaves it to the device.
    pub budget: Option<UNib32>,
}

#[cfg(feature = "std")]
impl ObserveRequest<'_> {
    pub fn make_owned(&self) -> Result<ObserveRequestOwned, Error> {
        Ok(ObserveRequestOwned {
            entries: self.entries.iter().collect::<Result<Vec<_>, _>>()?,
            patch: self.patch,
            budget: self.budget,
        })
    }
}

/// Answer to [RequestKind::Observe](crate::RequestKind::Observe), in an
/// [EventKind::Value](crate::EventKind::Value): the level's settings as the device applies them now.
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, Clone))]
pub struct ObserveGranted<'i> {
    /// Every property of the level that is observed now (not `Off`), with its period clamped to what the device can
    /// do, in index order.
    pub entries: RefVec<'i, ObserveEntry>,
    /// Entries of the request that were not applied, and why.
    pub refused: RefVec<'i, ObserveRefused>,
    /// Bytes per second the level's updates may take, as granted.
    pub budget: UNib32,
}

/// An entry of an [ObserveRequest] the device did not apply.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq)
)]
pub struct ObserveRefused {
    pub index: UNib32,
    pub reason: ObserveRefusal,
}

/// Why an entry of an [ObserveRequest] was not applied.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq),
    ww_repr = u4
)]
pub enum ObserveRefusal {
    /// No resource with this index in the level.
    NoSuchIndex,
    /// The resource is not a property, or a property without `+observe`.
    NotObservable,
    /// A mode this device doesn't know or doesn't support.
    ModeNotSupported,
    /// The index was listed more than once; the first entry was applied.
    Duplicate,
}

/// Serialize an [EventKind::PropertyUpdate](crate::EventKind::PropertyUpdate) event of the level at `path` into
/// `scratch`: `data` writes the level's properties (an `OptionAnyVec` by resource index) at the start of the buffer
/// it is given and returns their length. Same bytes as serializing the event with the data in place.
pub fn ser_property_update_event<'a>(
    scratch: &'a mut [u8],
    path: &[UNib32],
    t: Option<Second>,
    missed: Option<UNib32>,
    data: impl FnOnce(&mut [u8]) -> Result<usize, Error>,
) -> Result<&'a [u8], Error> {
    use crate::EventKindDiscriminants;
    use crate::builder::{EventBuilder, EventKindBuilder};
    let mut header_buf = [0u8; 64];
    let mut wr = BufWriter::new(&mut header_buf);
    let event_builder = EventBuilder::new(0, &mut wr)?;
    let event_kind_builder = EventKindBuilder::new(&mut wr)?;
    wr.write(&RefVec::Slice { slice: path })?;
    t.ser_shrink_wrap(&mut wr)?;
    missed.ser_shrink_wrap(&mut wr)?;
    event_kind_builder.finish_with_kind(EventKindDiscriminants::PropertyUpdate, &mut wr);
    event_builder.finish(true, &mut wr);
    wr.align_byte();
    let head_len = wr.pos().0;
    let header = wr.finish_and_take()?;
    if head_len > scratch.len() {
        return Err(Error::OutOfBoundsWriteRawSlice);
    }
    let data_len = data(&mut scratch[head_len..])?;
    let sizes = &header[head_len..];
    let end = head_len + data_len + sizes.len();
    if end > scratch.len() {
        return Err(Error::OutOfBoundsWriteRawSlice);
    }
    scratch[..head_len].copy_from_slice(&header[..head_len]);
    scratch[head_len + data_len..end].copy_from_slice(sizes);
    Ok(&scratch[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Event, EventKind, PathKind, Request, RequestKind};
    use wire_weaver::shrink_wrap::tail_bytes::TailBytes;
    use wire_weaver::shrink_wrap::{DeserializeShrinkWrap, UVlq32, UVlq32Backfill};
    use ww_si::Prefix;
    use ww_si::ww_numeric::NumericValue;

    const MICROS: Second = Second {
        prefix: Prefix::Micro,
        value: NumericValue::U32(0x1234_5678),
    };

    #[test]
    fn request_round_trip() {
        let entries = [
            ObserveEntry {
                index: UNib32(0),
                mode: ObserveMode::Periodic {
                    every_us: UNib32(40),
                },
            },
            ObserveEntry {
                index: UNib32(3),
                mode: ObserveMode::OnChangeOrBeat {
                    min_us: UNib32(1_000),
                    max_us: UNib32(1_000_000),
                },
            },
        ];
        let path = [UNib32(1)];
        let req = Request {
            seq: UVlq32Backfill(5),
            path_kind: PathKind::absolute(&path),
            kind: RequestKind::Observe {
                cfg: ObserveRequest {
                    entries: RefVec::Slice { slice: &entries },
                    patch: false,
                    budget: Some(UNib32(20_000)),
                },
            },
        };
        let mut buf = [0u8; 64];
        let b = req.to_ww_bytes(&mut buf).unwrap().to_vec();
        let back = Request::from_ww_bytes(&b).unwrap();
        let RequestKind::Observe { cfg } = back.kind else {
            panic!("{back:?}");
        };
        let back_entries: Vec<_> = cfg.entries.iter().map(|e| e.unwrap()).collect();
        assert_eq!(back_entries, entries);
        assert!(!cfg.patch);
        assert_eq!(cfg.budget, Some(UNib32(20_000)));
        // 5 of them are the seq placeholder, sent as 1 (Request::set_seq)
        assert_eq!(b.len(), 26, "{b:02x?}");
    }

    #[test]
    fn granted_round_trip() {
        let entries = [ObserveEntry {
            index: UNib32(2),
            mode: ObserveMode::OnChange {
                min_us: UNib32(100),
            },
        }];
        let refused = [ObserveRefused {
            index: UNib32(7),
            reason: ObserveRefusal::NotObservable,
        }];
        let granted = ObserveGranted {
            entries: RefVec::Slice { slice: &entries },
            refused: RefVec::Slice { slice: &refused },
            budget: UNib32(1000),
        };
        let mut buf = [0u8; 64];
        let b = granted.to_ww_bytes(&mut buf).unwrap().to_vec();
        let back = ObserveGranted::from_ww_bytes(&b).unwrap();
        assert_eq!(back.entries.iter().next().unwrap().unwrap(), entries[0]);
        assert_eq!(back.refused.iter().next().unwrap().unwrap(), refused[0]);
        assert_eq!(back.budget, UNib32(1000));
    }

    #[test]
    fn unknown_mode_is_read() {
        // a newer host's mode reads as Unknown, so the device can refuse just that entry
        let mut buf = [0u8; 16];
        let mode = ObserveMode::Unknown(9);
        let b = mode.to_ww_bytes(&mut buf).unwrap().to_vec();
        assert_eq!(
            ObserveMode::from_ww_bytes(&b).unwrap(),
            ObserveMode::Unknown(9)
        );
    }

    #[test]
    fn update_event_in_place() {
        let path = [UNib32(2)];
        let data = [0xaa, 0xbb, 0xcc];
        let event = Event {
            seq: UVlq32(0),
            result: Ok(EventKind::PropertyUpdate {
                path: RefVec::Slice { slice: &path },
                t: Some(MICROS),
                missed: Some(UNib32(3)),
                data: TailBytes(&data),
            }),
        };
        let mut buf = [0u8; 64];
        let expected = event.to_ww_bytes(&mut buf).unwrap().to_vec();

        let mut scratch = [0u8; 64];
        let built =
            ser_property_update_event(&mut scratch, &path, Some(MICROS), Some(UNib32(3)), |buf| {
                buf[..3].copy_from_slice(&data);
                Ok(3)
            })
            .unwrap();
        assert_eq!(built, expected);
        let back = Event::from_ww_bytes(built).unwrap();
        let Ok(EventKind::PropertyUpdate {
            path: p,
            t,
            missed,
            data: d,
        }) = back.result
        else {
            panic!("{back:?}");
        };
        assert_eq!(p.iter().next().unwrap().unwrap(), UNib32(2));
        assert!(t.is_some());
        assert_eq!(missed, Some(UNib32(3)));
        assert_eq!(d.0, data);
    }

    /// Without a timestamp and nothing missed, an update of a level one deep costs 4 bytes plus its data.
    #[test]
    fn update_event_overhead() {
        let path = [UNib32(2)];
        let mut scratch = [0u8; 64];
        let built = ser_property_update_event(&mut scratch, &path, None, None, |buf| {
            buf[0] = 0xaa;
            Ok(1)
        })
        .unwrap();
        assert_eq!(built.len(), 4 + 1, "{built:02x?}");
    }
}

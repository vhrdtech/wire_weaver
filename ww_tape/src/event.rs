//! The item of a record on an events track: its end and flags as the track's [EventsDef] says, then the payload.
//!
//! ```text
//! uvar  duration in ticks, end - start: only when the track's ends are Ends::Spans
//! uvar  flags (u32): only when the track has flags (EventsDef::flags)
//! ...   payload, to the end of the item
//! ```
//!
//! The start is the record's time. A track of plain instants ([EventsDef::INSTANTS]) carries the payload alone. The
//! same form is the item of a [Sticky](crate::Sticky) record of an events track, so a reader joining late gets the
//! whole event.

use crate::{Ends, EventsDef, uvar_len, varint};

/// An event as a record's item carries it: the start is the record's time.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct EventRecord<'p> {
    /// The end of a span, at or after the start; None for an instant. A spans track writes None as an instant
    /// (`end == start`) and reads it back as `Some(start)`.
    pub end: Option<i64>,
    /// The flags word; 0 on a track without flags.
    pub flags: u32,
    pub payload: &'p [u8],
}

/// Why an [EventRecord] can't be written or read.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum EventRecordError {
    /// An end before the start.
    EndBeforeStart { start: i64, end: i64 },
    /// An end on a track of instants.
    EndOnInstants,
    /// Flags other than 0 on a track without flags.
    FlagsWithoutFlags(u32),
    /// The track's [Ends] are a kind this reader doesn't know.
    UnknownEnds,
    /// The output is shorter than [EventRecord::encoded_len].
    Full { needed: usize },
    /// The item's duration or flags don't parse, or the end doesn't fit an i64.
    BadItem,
}

impl core::fmt::Display for EventRecordError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EndBeforeStart { start, end } => write!(f, "end {end} before start {start}"),
            Self::EndOnInstants => write!(f, "an end on a track of instants"),
            Self::FlagsWithoutFlags(v) => write!(f, "flags {v:#x} on a track without flags"),
            Self::UnknownEnds => write!(f, "the track's ends are of an unknown kind"),
            Self::Full { needed } => write!(f, "output shorter than {needed} bytes"),
            Self::BadItem => write!(f, "duration or flags don't parse"),
        }
    }
}

impl core::error::Error for EventRecordError {}

impl<'p> EventRecord<'p> {
    /// An instant without flags.
    pub const fn instant(payload: &'p [u8]) -> Self {
        EventRecord {
            end: None,
            flags: 0,
            payload,
        }
    }

    /// The duration and flags varints to write for a record starting at `start` on a track of `def`, checked.
    fn fields(
        &self,
        def: EventsDef,
        start: i64,
    ) -> Result<(Option<u64>, Option<u32>), EventRecordError> {
        let duration = match (def.ends, self.end) {
            (Ends::Instants, None) => None,
            (Ends::Instants, Some(_)) => return Err(EventRecordError::EndOnInstants),
            (Ends::Spans, end) => {
                let end = end.unwrap_or(start);
                let d = i128::from(end) - i128::from(start);
                Some(
                    u64::try_from(d)
                        .map_err(|_| EventRecordError::EndBeforeStart { start, end })?,
                )
            }
            (Ends::Unknown(_), _) => return Err(EventRecordError::UnknownEnds),
        };
        let flags = match (def.flags, self.flags) {
            (true, v) => Some(v),
            (false, 0) => None,
            (false, v) => return Err(EventRecordError::FlagsWithoutFlags(v)),
        };
        Ok((duration, flags))
    }

    /// Bytes of the item for a record starting at `start` on a track of `def`.
    pub fn encoded_len(&self, def: EventsDef, start: i64) -> Result<usize, EventRecordError> {
        let (d, f) = self.fields(def, start)?;
        Ok(d.map_or(0, uvar_len) + f.map_or(0, |f| uvar_len(u64::from(f))) + self.payload.len())
    }

    /// Write the item for a record starting at `start` on a track of `def` into `out`; returns its length. Fits
    /// [BlockWriter::push_with](crate::BlockWriter::push_with).
    pub fn encode(
        &self,
        def: EventsDef,
        start: i64,
        out: &mut [u8],
    ) -> Result<usize, EventRecordError> {
        let needed = self.encoded_len(def, start)?;
        if out.len() < needed {
            return Err(EventRecordError::Full { needed });
        }
        let (d, f) = self.fields(def, start)?;
        let mut at = 0;
        if let Some(d) = d {
            at += varint::put_uvar(&mut out[at..], d);
        }
        if let Some(f) = f {
            at += varint::put_uvar(&mut out[at..], u64::from(f));
        }
        out[at..needed].copy_from_slice(self.payload);
        Ok(needed)
    }

    /// Read the item `item` of a record starting at `start` on a track of `def`.
    pub fn decode(def: EventsDef, start: i64, item: &'p [u8]) -> Result<Self, EventRecordError> {
        let mut rd = varint::Reader(item);
        let end = match def.ends {
            Ends::Instants => None,
            Ends::Spans => {
                let d = rd.uvar().ok_or(EventRecordError::BadItem)?;
                Some(
                    start
                        .checked_add_unsigned(d)
                        .ok_or(EventRecordError::BadItem)?,
                )
            }
            Ends::Unknown(_) => return Err(EventRecordError::UnknownEnds),
        };
        let flags = if def.flags {
            let v = rd.uvar().ok_or(EventRecordError::BadItem)?;
            u32::try_from(v).map_err(|_| EventRecordError::BadItem)?
        } else {
            0
        };
        Ok(EventRecord {
            end,
            flags,
            payload: rd.0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPANS_FLAGS: EventsDef = EventsDef {
        ends: Ends::Spans,
        flags: true,
    };
    const SPANS: EventsDef = EventsDef {
        ends: Ends::Spans,
        flags: false,
    };
    const FLAGS: EventsDef = EventsDef {
        ends: Ends::Instants,
        flags: true,
    };

    fn round(def: EventsDef, start: i64, e: EventRecord<'_>) -> Vec<u8> {
        let mut out = vec![0u8; e.encoded_len(def, start).unwrap()];
        assert_eq!(e.encode(def, start, &mut out), Ok(out.len()));
        let back = EventRecord::decode(def, start, &out).unwrap();
        let want_end = match def.ends {
            Ends::Spans => Some(e.end.unwrap_or(start)),
            _ => None,
        };
        assert_eq!(back, EventRecord { end: want_end, ..e });
        out
    }

    #[test]
    fn items_carry_what_the_track_has() {
        let e = EventRecord {
            end: Some(1300),
            flags: 0x81,
            payload: b"ab",
        };
        // duration 300 (2 bytes), flags 0x81 (2 bytes), payload
        assert_eq!(
            round(SPANS_FLAGS, 1000, e),
            [0xac, 0x02, 0x81, 0x01, b'a', b'b']
        );
        assert_eq!(
            round(SPANS, 1000, EventRecord { flags: 0, ..e }),
            [0xac, 0x02, b'a', b'b']
        );
        assert_eq!(
            round(FLAGS, 1000, EventRecord { end: None, ..e }),
            [0x81, 0x01, b'a', b'b']
        );
        // plain instants: the payload alone
        assert_eq!(
            round(EventsDef::INSTANTS, 5, EventRecord::instant(b"ab")),
            b"ab"
        );
        // an instant on a spans track, and the widest span
        round(SPANS, -7, EventRecord::instant(b""));
        round(
            SPANS,
            i64::MIN,
            EventRecord {
                end: Some(i64::MAX),
                flags: 0,
                payload: b"x",
            },
        );
        round(
            SPANS_FLAGS,
            0,
            EventRecord {
                end: Some(0),
                flags: u32::MAX,
                payload: b"",
            },
        );
    }

    #[test]
    fn what_the_track_does_not_have_is_refused() {
        let mut out = [0u8; 16];
        let e = EventRecord::instant(b"p");
        assert_eq!(
            EventRecord { end: Some(9), ..e }.encode(EventsDef::INSTANTS, 0, &mut out),
            Err(EventRecordError::EndOnInstants)
        );
        assert_eq!(
            EventRecord { flags: 4, ..e }.encode(SPANS, 0, &mut out),
            Err(EventRecordError::FlagsWithoutFlags(4))
        );
        assert_eq!(
            EventRecord { end: Some(9), ..e }.encode(SPANS, 10, &mut out),
            Err(EventRecordError::EndBeforeStart { start: 10, end: 9 })
        );
        let unknown = EventsDef {
            ends: Ends::Unknown(9),
            flags: false,
        };
        assert_eq!(
            e.encode(unknown, 0, &mut out),
            Err(EventRecordError::UnknownEnds)
        );
        assert_eq!(
            EventRecord::decode(unknown, 0, b"p"),
            Err(EventRecordError::UnknownEnds)
        );
        assert_eq!(
            EventRecord {
                end: Some(300),
                ..e
            }
            .encode(SPANS, 0, &mut out[..2]),
            Err(EventRecordError::Full { needed: 3 })
        );
    }

    #[test]
    fn bad_items_are_refused() {
        // no duration, a cut varint, flags over u32, an end past i64
        assert_eq!(
            EventRecord::decode(SPANS, 0, b""),
            Err(EventRecordError::BadItem)
        );
        assert_eq!(
            EventRecord::decode(SPANS, 0, &[0x80]),
            Err(EventRecordError::BadItem)
        );
        assert_eq!(
            EventRecord::decode(FLAGS, 0, &[0x80, 0x80, 0x80, 0x80, 0x10]),
            Err(EventRecordError::BadItem)
        );
        assert_eq!(
            EventRecord::decode(SPANS, i64::MAX, &[1]),
            Err(EventRecordError::BadItem)
        );
    }
}

//! Wire types of counted and reliable streams (API-30), see `docs/api/stream_reliability.md`.
//!
//! The state machines that use them live in the `ww_stream` crate.

use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::tail_bytes::TailBytes;
use wire_weaver::shrink_wrap::{
    BufReader, BufWriter, DeserializeShrinkWrap, ElementSize, Error, SerializeShrinkWrap,
};
#[cfg(feature = "std")]
use wire_weaver::shrink_wrap::{
    BufWriterOwned, DeserializeShrinkWrapOwned, SerializeShrinkWrapOwned,
    tail_bytes::TailBytesOwned,
};
use ww_si::Second;

/// Per-event head of a counted or reliable stream ([EventKind::StreamItem](crate::EventKind::StreamItem)): where the
/// event's data sits in the stream, how much the device dropped right before it and when it was made.
///
/// Positions count units the device names when it sends (bytes, transactions, sample sets), from the start of the
/// session. The next event's `pos` is `pos + units` of this one plus its own `lost`, so the host tells device drops
/// (`lost`), link losses (a jump `lost` doesn't explain) and duplicates (`pos` lower than expected) apart without
/// knowing anything about the items.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StreamHead {
    /// Position of the event's first unit since the session started. Below 2⁵⁷, see [StreamHead::MAX_POS].
    pub pos: u64,
    /// Units of the event's item, as the device counts them; 1 costs nothing on the wire.
    pub units: u64,
    /// Units the device dropped right before this event; `pos` counts them.
    pub lost: Option<u64>,
    /// Device time of the event's first unit (`ww_timebase` clock).
    pub t: Option<Second>,
    /// Rare markers.
    pub ext: Option<HeadExt>,
}

impl StreamHead {
    /// Largest position the head can carry: 1 bit in the first byte and up to 7 more bytes.
    pub const MAX_POS: u64 = (1 << 57) - 1;

    /// Head of an event of one unit with a position only.
    pub const fn at(pos: u64) -> Self {
        StreamHead {
            pos,
            units: 1,
            lost: None,
            t: None,
            ext: None,
        }
    }

    /// `lost` as a number, 0 when not there.
    pub fn lost_units(&self) -> u64 {
        self.lost.unwrap_or(0)
    }

    /// Position right after the event's units: where the next event starts when nothing is lost in between.
    pub fn end(&self) -> u64 {
        self.pos + self.units
    }

    /// Position where the previous event ended when nothing was lost on the link: `pos - lost`.
    pub fn after_previous(&self) -> u64 {
        self.pos.saturating_sub(self.lost_units())
    }
}

/// Rare markers of a [StreamHead], evolvable: fields can be appended later.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq, Default)
)]
pub struct HeadExt {
    /// The device's clock restarted (a reboot) before this event: timestamps from here on belong to this epoch.
    pub clock_epoch: Option<UVlq32>,
}

/// A point in a stream: a position in a session. The host keeps one per open stream and passes it back to resume
/// after a reconnect or a restart ([RequestKind::StreamOpen](crate::RequestKind::StreamOpen)).
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq),
    final_structure
)]
pub struct StreamCursor {
    /// Session as numbered by the device.
    pub session: UVlq32,
    /// Position: every unit before it was received (or reported lost).
    pub pos: UVlq64,
}

/// Answer to [RequestKind::StreamOpen](crate::RequestKind::StreamOpen), in an
/// [EventKind::Value](crate::EventKind::Value).
///
/// Live events start at `live_from`; backfill events (resumed reliable streams only) run from `resume_from` up to
/// `live_from`. Units between the resume cursor and `resume_from` (or `live_from` when there is no backfill) are
/// gone: the device didn't keep them.
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug), final_structure)]
pub struct StreamOpened<'i> {
    /// Session the stream is in now: a new one, or the cursor's when resumed.
    pub session: UVlq32,
    /// Oldest position the device still has; `live_from` when it keeps nothing (counted streams).
    pub oldest: UVlq64,
    /// Where the live run starts: at the newest event the device has (so a slow stream shows its latest value at
    /// once), or at the units the device lost right before it, or at the next event when it keeps nothing newer
    /// than the cursor.
    pub live_from: UVlq64,
    /// Where the backfill run starts, `None` when there is no backfill.
    pub resume_from: Option<UVlq64>,
    /// Batched streams (API-41): the most a batch waits on the device before it is sent, in µs, granted for the
    /// `max_age_us` of [StreamOpen](crate::RequestKind::StreamOpen); `None` for a stream that isn't batched.
    pub max_age_us: Option<UNib32>,
    /// Serialized result of the stream function (its `Result<T, E>`), empty for `stream!`.
    pub result: TailBytes<'i>,
}

impl StreamOpened<'_> {
    /// With the `max_age_us` a batched stream granted (API-41).
    pub fn with_max_age_us(mut self, max_age_us: Option<u32>) -> Self {
        self.max_age_us = max_age_us.map(UNib32);
        self
    }

    /// Write everything but `result`, byte aligned, so that the stream function's result can follow in place,
    /// serialized with its own sizes at the back of the buffer, as a method's return value is in
    /// [EventKind::Value](crate::EventKind::Value). Same bytes as serializing `self` with the result in `result`.
    pub fn ser_head(&self, wr: &mut BufWriter) -> Result<(), Error> {
        wr.align_byte();
        wr.write(&self.session)?;
        wr.write(&self.oldest)?;
        wr.write(&self.live_from)?;
        wr.write(&self.resume_from)?;
        wr.write(&self.max_age_us)?;
        wr.align_byte();
        Ok(())
    }
}

/// Serialize an [EventKind::StreamItem](crate::EventKind::StreamItem) event of the stream at `path` into `scratch`:
/// `item` writes the item's bytes at the start of the buffer it is given (`value.to_ww_bytes(buf)`, or raw bytes as
/// a store keeps them) and returns their length. The item keeps its own sizes, as `from_ww_bytes` on the host
/// expects. Used by generated `stream_data_ser()` methods and `poll_streams`.
pub fn ser_stream_item_event<'a>(
    scratch: &'a mut [u8],
    path: &[UNib32],
    head: &StreamHead,
    item: impl FnOnce(&mut [u8]) -> Result<usize, Error>,
) -> Result<&'a [u8], Error> {
    use crate::EventKindDiscriminants;
    use crate::builder::{EventBuilder, EventKindBuilder};
    let mut header_buf = [0u8; 128];
    let mut wr = BufWriter::new(&mut header_buf);
    let event_builder = EventBuilder::new(0, &mut wr)?;
    let event_kind_builder = EventKindBuilder::new(&mut wr)?;
    wr.write(&RefVec::Slice { slice: path })?;
    wr.write(head)?;
    event_kind_builder.finish_with_kind(EventKindDiscriminants::StreamItem, &mut wr);
    event_builder.finish(true, &mut wr);
    wr.align_byte();
    let head_len = wr.pos().0;
    let header = wr.finish_and_take()?;
    if head_len > scratch.len() {
        return Err(Error::OutOfBoundsWriteRawSlice);
    }
    let item_len = item(&mut scratch[head_len..])?;
    let sizes = &header[head_len..];
    let end = head_len + item_len + sizes.len();
    if end > scratch.len() {
        return Err(Error::OutOfBoundsWriteRawSlice);
    }
    scratch[..head_len].copy_from_slice(&header[..head_len]);
    scratch[head_len + item_len..end].copy_from_slice(sizes);
    Ok(&scratch[..end])
}

/// State of a stream, the answer to a [RequestKind::Read](crate::RequestKind::Read) on its path (API-31): whether a
/// host has it open, its session and positions, and what its store holds. Evolvable, fields may be appended.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq)
)]
pub struct StreamStatus {
    /// A host has the stream open (counted and reliable streams stay attached across a link loss only until the
    /// device notices it).
    pub open: bool,
    /// Current session, `None` before the first one.
    pub session: Option<UVlq32>,
    /// Position of the next unit.
    pub next: UVlq64,
    /// Oldest and newest positions kept in the store, `None` when it is empty or there is no store.
    pub oldest: Option<UVlq64>,
    pub newest: Option<UVlq64>,
    /// Bytes sent and not acknowledged yet (reliable streams).
    pub in_flight: UVlq64,
    /// What the store holds, `None` without one (plain and counted streams).
    pub store: Option<StoreStatus>,
}

/// Store part of a [StreamStatus].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, Clone, Copy, PartialEq, Eq)
)]
pub struct StoreStatus {
    /// Bytes in use, including the store's own overhead.
    pub used_bytes: UVlq64,
    /// Bytes the store can hold.
    pub capacity_bytes: UVlq64,
    /// Survives a reboot (a log on SD or flash), not just a reconnect.
    pub persistent: bool,
    /// Units the store forgot to make room (drop oldest) in this session.
    pub dropped_units: UVlq64,
}

impl StreamHead {
    fn encode_pos(&self) -> Result<([u8; 8], usize), Error> {
        if self.pos > Self::MAX_POS {
            return Err(Error::SubtypeOutOfRange);
        }
        let n = if self.pos < 2 {
            0
        } else {
            (64 - self.pos.leading_zeros() as usize - 1).div_ceil(8)
        };
        let mut bytes = [0u8; 8];
        let mut flags = 0u8;
        if self.lost.is_some() {
            flags |= 0x80;
        }
        if self.t.is_some() {
            flags |= 0x40;
        }
        if self.ext.is_some() {
            flags |= 0x20;
        }
        if self.units != 1 {
            flags |= 0x10;
        }
        bytes[0] = flags | ((n as u8) << 1) | ((self.pos >> (8 * n)) & 1) as u8;
        for i in 0..n {
            bytes[1 + i] = (self.pos >> (8 * (n - 1 - i))) as u8;
        }
        Ok((bytes, n + 1))
    }

    /// Bytes of the head without `t` and `ext`: the first byte, the position, `units` and `lost`.
    pub fn len_without_t_ext(&self) -> usize {
        let pos_len = match self.encode_pos() {
            Ok((_, len)) => len,
            Err(_) => 8,
        };
        let units_len = if self.units != 1 {
            UVlq64(self.units).len_bytes()
        } else {
            0
        };
        pos_len + units_len + self.lost.map(|l| UVlq64(l).len_bytes()).unwrap_or(0)
    }

    fn decode_pos(rd: &mut BufReader<'_>) -> Result<(u8, u64), Error> {
        let b0 = rd.read_u8()?;
        let n = ((b0 >> 1) & 0b111) as usize;
        let mut pos = (b0 & 1) as u64;
        for &b in rd.read_raw_slice(n)? {
            pos = (pos << 8) | b as u64;
        }
        Ok((b0, pos))
    }
}

// Layout ("Head on the wire" in docs/api/stream_reliability.md):
//  byte 0:  L T X U n n n p   L, T, X, U: lost, t, ext, units (when not 1) follow; nnn: bytes of pos after this
//                              one; p: top bit of pos
//  then n bytes of pos (big endian), units: UVlq64, lost: UVlq64, t: Second, ext: HeadExt (Unsized, its length in
//  the back).
impl SerializeShrinkWrap for StreamHead {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn ser_shrink_wrap(&self, wr: &mut BufWriter) -> Result<(), Error> {
        let (bytes, len) = self.encode_pos()?;
        wr.write_raw_slice(&bytes[..len])?;
        if self.units != 1 {
            wr.write_uvlq64(self.units)?;
        }
        if let Some(lost) = self.lost {
            wr.write_uvlq64(lost)?;
        }
        if let Some(t) = &self.t {
            wr.write(t)?;
        }
        if let Some(ext) = &self.ext {
            wr.write(ext)?;
        }
        Ok(())
    }
}

#[cfg(feature = "std")]
impl SerializeShrinkWrapOwned for StreamHead {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn ser_shrink_wrap_owned(&self, wr: &mut BufWriterOwned) -> Result<(), Error> {
        let (bytes, len) = self.encode_pos()?;
        wr.write_raw_slice(&bytes[..len])?;
        if self.units != 1 {
            wr.write_uvlq64(self.units)?;
        }
        if let Some(lost) = self.lost {
            wr.write_uvlq64(lost)?;
        }
        if let Some(t) = &self.t {
            wr.write(t)?;
        }
        if let Some(ext) = &self.ext {
            wr.write(ext)?;
        }
        Ok(())
    }
}

impl<'i> DeserializeShrinkWrap<'i> for StreamHead {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn des_shrink_wrap<'di>(rd: &'di mut BufReader<'i>) -> Result<Self, Error> {
        let (b0, pos) = Self::decode_pos(rd)?;
        let units = if b0 & 0x10 != 0 { rd.read_uvlq64()? } else { 1 };
        let lost = if b0 & 0x80 != 0 {
            Some(rd.read_uvlq64()?)
        } else {
            None
        };
        let t = if b0 & 0x40 != 0 {
            Some(rd.read()?)
        } else {
            None
        };
        let ext = if b0 & 0x20 != 0 {
            Some(rd.read()?)
        } else {
            None
        };
        Ok(StreamHead {
            pos,
            units,
            lost,
            t,
            ext,
        })
    }
}

#[cfg(feature = "std")]
impl DeserializeShrinkWrapOwned for StreamHead {
    const ELEMENT_SIZE: ElementSize = ElementSize::UnsizedFinalStructure;

    fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, Error> {
        let (b0, pos) = Self::decode_pos(rd)?;
        let units = if b0 & 0x10 != 0 { rd.read_uvlq64()? } else { 1 };
        let lost = if b0 & 0x80 != 0 {
            Some(rd.read_uvlq64()?)
        } else {
            None
        };
        let t = if b0 & 0x40 != 0 {
            Some(rd.read_owned()?)
        } else {
            None
        };
        let ext = if b0 & 0x20 != 0 {
            Some(rd.read_owned()?)
        } else {
            None
        };
        Ok(StreamHead {
            pos,
            units,
            lost,
            t,
            ext,
        })
    }
}

#[cfg(test)]
// binary literals grouped as the fields of byte 0: L T X U, nnn, p
#[allow(clippy::unusual_byte_groupings)]
mod tests {
    use super::*;
    use crate::{Event, EventKind};
    use ww_si::Prefix;
    use ww_si::ww_numeric::NumericValue;

    fn bytes(head: &StreamHead) -> std::vec::Vec<u8> {
        let mut buf = [0u8; 64];
        let bytes = head.to_ww_bytes(&mut buf).unwrap().to_vec();
        assert_eq!(
            StreamHead::from_ww_bytes(&bytes).unwrap(),
            *head,
            "{bytes:02x?}"
        );
        assert_eq!(StreamHead::from_ww_bytes_owned(&bytes).unwrap(), *head);
        assert_eq!(head.to_ww_bytes_owned().unwrap(), bytes);
        bytes
    }

    const MICROS: Second = Second::from_parts(Prefix::Micro, NumericValue::U32(0x1234_5678));

    #[test]
    fn pos_only() {
        assert_eq!(bytes(&StreamHead::at(0)), [0x00]);
        assert_eq!(bytes(&StreamHead::at(1)), [0x01]);
        assert_eq!(bytes(&StreamHead::at(2)), [0b0000_001_0, 0x02]);
        assert_eq!(bytes(&StreamHead::at(0x1ff)), [0b0000_001_1, 0xff]);
        assert_eq!(bytes(&StreamHead::at(0x200)), [0b0000_010_0, 0x02, 0x00]);
        assert_eq!(
            bytes(&StreamHead::at(0x0123_4567_89ab_cdef)),
            [0b0000_111_1, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef]
        );
        assert_eq!(bytes(&StreamHead::at(StreamHead::MAX_POS)).len(), 8);
    }

    #[test]
    fn pos_above_max_is_an_error() {
        let mut buf = [0u8; 16];
        assert_eq!(
            StreamHead::at(StreamHead::MAX_POS + 1).to_ww_bytes(&mut buf),
            Err(Error::SubtypeOutOfRange)
        );
    }

    /// The table in docs/api/stream_reliability.md, "Head on the wire".
    #[test]
    fn sizes_by_position() {
        let limits = [
            (1u64 << 1, 1),
            (1 << 9, 2),
            (1 << 17, 3),
            (1 << 25, 4),
            (1 << 33, 5),
            (1 << 41, 6),
            (1 << 49, 7),
            (1 << 57, 8),
        ];
        let mut prev = 0;
        for (below, len) in limits {
            assert_eq!(
                bytes(&StreamHead::at(below - 1)).len(),
                len,
                "below {below}"
            );
            assert_eq!(bytes(&StreamHead::at(prev)).len(), len, "at {prev}");
            prev = below;
        }
    }

    #[test]
    fn units() {
        let head = |units| StreamHead {
            units,
            ..StreamHead::at(5)
        };
        assert_eq!(bytes(&head(1)), [0b0000_001_0, 0x05]);
        assert_eq!(bytes(&head(0)), [0b0001_001_0, 0x05, 0]);
        assert_eq!(bytes(&head(127)), [0b0001_001_0, 0x05, 127]);
        assert_eq!(bytes(&head(128)), [0b0001_001_0, 0x05, 0x81, 0x00]);
        assert_eq!(bytes(&head(16383)).len(), 4);
        assert_eq!(bytes(&head(16384)).len(), 5);
        assert_eq!(bytes(&head(u64::MAX)).len(), 12);
        assert_eq!(head(37).end(), 42);
    }

    #[test]
    fn lost_t_ext() {
        let head = StreamHead {
            lost: Some(100),
            ..StreamHead::at(5)
        };
        assert_eq!(bytes(&head), [0b1000_001_0, 0x05, 100]);
        assert_eq!(head.after_previous(), 0);
        let head = StreamHead {
            lost: Some(200),
            units: 300,
            ..head
        };
        // units before lost
        assert_eq!(bytes(&head), [0b1001_001_0, 0x05, 0x82, 0x2c, 0x81, 0x48]);
        assert_eq!(head.len_without_t_ext(), 6);

        let head = StreamHead {
            t: Some(MICROS),
            ..StreamHead::at(5)
        };
        // Second { Micro, U32 }: prefix and number type in one byte, then the u32
        assert_eq!(
            bytes(&head),
            [0b0100_001_0, 0x05, 0x23, 0x78, 0x56, 0x34, 0x12]
        );

        let t64 = StreamHead {
            t: Some(Second::from_parts(
                Prefix::Micro,
                NumericValue::U64(1 << 40),
            )),
            ..head
        };
        assert_eq!(bytes(&t64).len(), 2 + 9);

        let ext = StreamHead {
            pos: 5,
            units: 2,
            lost: Some(1),
            t: Some(MICROS),
            ext: Some(HeadExt {
                clock_epoch: Some(UVlq32(3)),
            }),
        };
        let b = bytes(&ext);
        assert_eq!(b[..4], [0b1111_001_0, 0x05, 2, 1]);

        let empty_ext = StreamHead {
            ext: Some(HeadExt::default()),
            ..StreamHead::at(5)
        };
        bytes(&empty_ext);
    }

    /// A 32 GB byte stream near its end, events of 1 KiB: 8 bytes, 13 with a timestamp.
    #[test]
    fn big_store() {
        let head = StreamHead {
            units: 1024,
            ..StreamHead::at(32 << 30)
        };
        assert_eq!(bytes(&head).len(), 8);
        let head = StreamHead {
            t: Some(MICROS),
            ..head
        };
        assert_eq!(bytes(&head).len(), 13);
        assert_eq!(bytes(&StreamHead::at(32 << 30)).len(), 6);
    }

    #[test]
    fn in_an_event() {
        let path = [UNib32(2)];
        let head = StreamHead {
            pos: 1 << 33,
            units: 3,
            lost: Some(7),
            t: Some(MICROS),
            ext: None,
        };
        let event = Event {
            seq: UVlq32(0),
            result: Ok(EventKind::StreamItem {
                path: RefVec::Slice { slice: &path },
                head,
                data: TailBytes(&[1, 2, 3]),
            }),
        };
        let mut buf = [0u8; 64];
        let b = event.to_ww_bytes(&mut buf).unwrap().to_vec();
        let back = Event::from_ww_bytes(&b).unwrap();
        let Ok(EventKind::StreamItem {
            path: p,
            head: h,
            data,
        }) = back.result
        else {
            panic!("{back:?}");
        };
        assert_eq!(p.iter().next().unwrap().unwrap(), UNib32(2));
        assert_eq!(h, head);
        assert_eq!(data.0, [1, 2, 3]);
        // seq, result and kind, path and its length in the back: 4 bytes as for StreamData; head 6 + 1 + 1 + 5; data
        assert_eq!(b.len(), 4 + 13 + 3, "{b:02x?}");
        assert_eq!(
            b[3..16],
            [0xda, 0x02, 0, 0, 0, 0, 3, 7, 0x23, 0x78, 0x56, 0x34, 0x12]
        );

        let owned = crate::EventOwned::from_ww_bytes_owned(&b).unwrap();
        assert_eq!(owned.to_ww_bytes_owned().unwrap(), b);
    }

    #[test]
    fn cursor_and_opened() {
        let cursor = StreamCursor {
            session: UVlq32(1),
            pos: UVlq64(300),
        };
        let mut buf = [0u8; 32];
        let b = cursor.to_ww_bytes(&mut buf).unwrap();
        assert_eq!(b, [1, 0x82, 0x2c]);
        assert_eq!(StreamCursor::from_ww_bytes(b).unwrap(), cursor);

        let opened = StreamOpened {
            session: UVlq32(1),
            oldest: UVlq64(0),
            live_from: UVlq64(1000),
            resume_from: Some(UVlq64(10)),
            max_age_us: None,
            result: TailBytes(&[0xaa]),
        };
        let b = opened.to_ww_bytes(&mut buf).unwrap().to_vec();
        let back = StreamOpened::from_ww_bytes(&b).unwrap();
        assert_eq!(back.live_from, UVlq64(1000));
        assert_eq!(back.resume_from, Some(UVlq64(10)));
        assert_eq!(back.result.0, [0xaa]);
    }

    #[test]
    fn stream_item_event_matches_derived() {
        let path = [UNib32(1), UNib32(12)];
        let head = StreamHead {
            units: 3,
            lost: Some(2),
            ..StreamHead::at(1000)
        };
        let item: (u8, &str) = (5, "hello");
        let mut ibuf = [0u8; 32];
        let item_bytes = item.to_ww_bytes(&mut ibuf).unwrap().to_vec();
        let event = Event {
            seq: UVlq32(0),
            result: Ok(EventKind::StreamItem {
                path: RefVec::Slice { slice: &path },
                head,
                data: TailBytes(&item_bytes),
            }),
        };
        let mut buf = [0u8; 64];
        let derived = event.to_ww_bytes(&mut buf).unwrap().to_vec();
        let mut buf = [0u8; 64];
        let built = ser_stream_item_event(&mut buf, &path, &head, |b| {
            item.to_ww_bytes(b).map(|b| b.len())
        })
        .unwrap();
        assert_eq!(built, derived);
        let Ok(EventKind::StreamItem { data, .. }) = Event::from_ww_bytes(built).unwrap().result
        else {
            panic!()
        };
        assert_eq!(<(u8, &str)>::from_ww_bytes(data.0).unwrap(), item);
    }

    #[test]
    fn opened_head_then_result_in_place() {
        let result: Result<(u8, &str), u8> = Ok((7, "abc"));
        let mut rbuf = [0u8; 32];
        let result_bytes = result.to_ww_bytes(&mut rbuf).unwrap().to_vec();
        let opened = StreamOpened {
            session: UVlq32(3),
            oldest: UVlq64(1),
            live_from: UVlq64(300),
            resume_from: Some(UVlq64(2)),
            max_age_us: Some(UNib32(20_000)),
            result: TailBytes(&result_bytes),
        };
        let mut buf = [0u8; 64];
        let derived = opened.to_ww_bytes(&mut buf).unwrap().to_vec();

        let mut buf = [0u8; 64];
        let mut wr = BufWriter::new(&mut buf);
        opened.ser_head(&mut wr).unwrap();
        result.ser_shrink_wrap(&mut wr).unwrap();
        let in_place = wr.finish_and_take().unwrap().to_vec();
        assert_eq!(in_place, derived);
        let back = StreamOpened::from_ww_bytes(&in_place).unwrap();
        assert_eq!(
            <Result<(u8, &str), u8>>::from_ww_bytes(back.result.0).unwrap(),
            result
        );

        let status = StreamStatus {
            open: true,
            session: Some(UVlq32(1)),
            next: UVlq64(10),
            oldest: None,
            newest: Some(UVlq64(9)),
            in_flight: UVlq64(0),
            store: Some(StoreStatus {
                used_bytes: UVlq64(5),
                capacity_bytes: UVlq64(4096),
                persistent: false,
                dropped_units: UVlq64(0),
            }),
        };
        let b = status.to_ww_bytes(&mut buf).unwrap().to_vec();
        assert_eq!(StreamStatus::from_ww_bytes(&b).unwrap(), status);
    }

    #[test]
    fn ack_sideband() {
        use crate::StreamSideband;
        let ack = StreamSideband::Ack {
            have: UVlq64(1 << 40),
            backfill: Some(UVlq64(5)),
            grant: UVlq32(4 << 20),
        };
        let mut buf = [0u8; 32];
        let b = ack.to_ww_bytes(&mut buf).unwrap().to_vec();
        assert_eq!(StreamSideband::from_ww_bytes(&b).unwrap(), ack);
        let q = StreamSideband::Query {
            sent: UVlq64(3),
            backfill: None,
        };
        let b = q.to_ww_bytes(&mut buf).unwrap().to_vec();
        assert_eq!(StreamSideband::from_ww_bytes(&b).unwrap(), q);
        let r = StreamSideband::ResendFrom {
            from: UVlq64(u64::MAX),
            grant: UVlq32(1),
        };
        let b = r.to_ww_bytes(&mut buf).unwrap().to_vec();
        assert_eq!(StreamSideband::from_ww_bytes(&b).unwrap(), r);
    }
}

//! Frames back into messages from their pieces ([ww_framer::pieces]), with the same rules `FramedRx` applies, and
//! for every message the frame bytes it was built from.

use std::marker::PhantomData;
use std::ops::Range;

use ww_framer::crc::{Crc16IbmSdlc, Crc32IsoHdlc, CrcChecksum};
use ww_framer::framed::U2Head;
use ww_framer::pieces::{Piece, PieceError, PieceErrorKind, pieces_with_prefix, verify_split};
use ww_framer::prefix::{FramePrefix, FrameReport, NopPrefix, SeqPrefix};
use ww_framer::traits::{Checksum, MessageKind, NopChecksum, NopTail, RdError, Tail};
use ww_tap::{Framer, FramerChecksum, FramerPrefix, FramerTail};

/// The framer a session names, as code, with what its frame prefix checks keep between frames.
pub(crate) trait Walker: Send {
    /// Bytes of the frame prefix.
    fn prefix_len(&self) -> usize;
    /// Check a frame's prefix as `FramedRx` does when it stages it.
    fn check_prefix(&mut self, frame: &[u8]) -> FrameReport;
    fn pieces(&self, frame: &[u8]) -> Vec<Result<Piece<u8>, PieceError>>;
    /// `prefix` and `end_head` are the End's frame prefix and head, covered by the checksum.
    fn verify_split(&self, prefix: &[u8], end_head: &[u8], message: &[u8], trailer: &[u8]) -> bool;
}

struct U2<C, T, P: FramePrefix> {
    rx: P::Rx,
    _phantom: PhantomData<fn() -> (C, T)>,
}

impl<C, T, P: FramePrefix> U2<C, T, P> {
    fn boxed() -> Box<Self> {
        Box::new(U2 {
            rx: Default::default(),
            _phantom: PhantomData,
        })
    }
}

impl<C: Checksum, T: Tail, P: FramePrefix> Walker for U2<C, T, P> {
    fn prefix_len(&self) -> usize {
        P::LEN
    }

    fn check_prefix(&mut self, frame: &[u8]) -> FrameReport {
        if frame.len() < P::LEN {
            return FrameReport::TooShort;
        }
        P::read(&mut self.rx, &frame[..P::LEN])
    }

    fn pieces(&self, frame: &[u8]) -> Vec<Result<Piece<u8>, PieceError>> {
        pieces_with_prefix::<U2Head, C, T, P>(frame).collect()
    }

    fn verify_split(&self, prefix: &[u8], end_head: &[u8], message: &[u8], trailer: &[u8]) -> bool {
        verify_split::<C>(prefix, end_head, message, trailer)
    }
}

/// The walker for a session's framer, None for media without frames; an error for a configuration this decoder
/// doesn't know.
/// A framer's walker and whether it is a stream medium.
pub(crate) type FramerCode = (Box<dyn Walker>, bool);

pub(crate) fn walker(framer: &Framer) -> Result<Option<FramerCode>, String> {
    let Framer::U2 {
        checksum,
        tail,
        stream,
        prefix,
    } = framer
    else {
        return Ok(None);
    };
    if !matches!(tail, FramerTail::None) {
        return Err(format!(
            "framer tail {tail:?} is not supported by this decoder"
        ));
    }
    if *stream && !matches!(prefix, FramerPrefix::None) {
        return Err(format!(
            "frame prefix {prefix:?} on a stream medium is not supported by this decoder"
        ));
    }
    let w = match prefix {
        FramerPrefix::None => with_checksum::<NopPrefix>(checksum)?,
        FramerPrefix::Seq => with_checksum::<SeqPrefix>(checksum)?,
        other => {
            return Err(format!(
                "frame prefix {other:?} is not supported by this decoder"
            ));
        }
    };
    Ok(Some((w, *stream)))
}

fn with_checksum<P: FramePrefix + 'static>(
    checksum: &FramerChecksum,
) -> Result<Box<dyn Walker>, String> {
    Ok(match checksum {
        FramerChecksum::None => U2::<NopChecksum, NopTail, P>::boxed(),
        FramerChecksum::Crc16IbmSdlc { full: false } => {
            U2::<CrcChecksum<Crc16IbmSdlc, false>, NopTail, P>::boxed()
        }
        FramerChecksum::Crc16IbmSdlc { full: true } => {
            U2::<CrcChecksum<Crc16IbmSdlc, true>, NopTail, P>::boxed()
        }
        FramerChecksum::Crc32IsoHdlc { full: false } => {
            U2::<CrcChecksum<Crc32IsoHdlc, false>, NopTail, P>::boxed()
        }
        FramerChecksum::Crc32IsoHdlc { full: true } => {
            U2::<CrcChecksum<Crc32IsoHdlc, true>, NopTail, P>::boxed()
        }
        other => {
            return Err(format!(
                "framer checksum {other:?} is not supported by this decoder"
            ));
        }
    })
}

/// Bytes of one frame a message was built from.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FramePart {
    pub frame_no: u64,
    /// Offset of the payload bytes in the frame.
    pub offset: u32,
    pub len: u32,
}

/// A message as the framer gives it back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reassembled {
    pub kind: u8,
    pub bytes: Vec<u8>,
    /// Frames it was built from, in order: one for a Full, two or more for a split message.
    pub parts: Vec<FramePart>,
}

/// What became of a piece.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PieceOutcome {
    /// Completes a message: a Full, or the End of a split one.
    Message,
    /// A Start or Continue of a message being assembled.
    Part,
    /// Dropped, and why.
    Dropped(&'static str),
}

/// One piece of a frame, located in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PieceInfo {
    pub kind: MessageKind,
    pub user_kind: u8,
    /// The head's length field.
    pub len: usize,
    pub head: Range<usize>,
    pub payload: Range<usize>,
    pub trailer: Range<usize>,
    pub outcome: PieceOutcome,
}

/// What a frame was made of.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FrameOutcome {
    pub pieces: Vec<PieceInfo>,
    /// Why the rest of the frame was skipped, from this offset on.
    pub error: Option<(usize, String)>,
    /// Messages completed by this frame, in order.
    pub completed: Vec<Reassembled>,
    /// A split message was dropped half way (a frame of it lost), with the bytes it had.
    pub dropped_partial: Option<usize>,
    /// The frame prefix (`0..0` without one, shorter if the frame is) and what its check found; a frame the check
    /// drops has no pieces.
    pub prefix: Range<usize>,
    pub prefix_report: Option<FrameReport>,
}

struct Partial {
    kind: u8,
    total: usize,
    bytes: Vec<u8>,
    parts: Vec<FramePart>,
}

/// One direction of a link: frames in, messages out.
pub(crate) struct Reassembler {
    walker: Box<dyn Walker>,
    stream: bool,
    partial: Option<Partial>,
    /// Stream media: bytes of chunks not yet cut into messages, and which chunk each run of them came from
    carry: Vec<u8>,
    /// (chunk no, start in `carry`, length, offset of that start in the chunk)
    carry_from: Vec<(u64, usize, usize, usize)>,
}

impl Reassembler {
    pub fn new(walker: Box<dyn Walker>, stream: bool) -> Self {
        Reassembler {
            walker,
            stream,
            partial: None,
            carry: Vec::new(),
            carry_from: Vec::new(),
        }
    }

    pub fn feed(&mut self, frame_no: u64, frame: &[u8]) -> FrameOutcome {
        if self.stream {
            return self.feed_stream(frame_no, frame);
        }
        let mut out = FrameOutcome::default();
        if self.walker.prefix_len() > 0 {
            let report = self.walker.check_prefix(frame);
            out.prefix_report = Some(report);
            out.prefix = 0..self.walker.prefix_len().min(frame.len());
            if report.is_dropped() {
                return out;
            }
            if report.breaks_message() {
                self.drop_partial(&mut out);
            }
        }
        let all = self.walker.pieces(frame);
        let part = |r: &Range<usize>| FramePart {
            frame_no,
            offset: r.start as u32,
            len: r.len() as u32,
        };
        'pieces: for p in all {
            let p = match p {
                Ok(p) => p,
                Err(e) => {
                    if matches!(e.kind, PieceErrorKind::BadSplit) {
                        self.drop_partial(&mut out);
                    }
                    out.error = Some((e.at, describe(&e)));
                    break;
                }
            };
            let payload = &frame[p.payload.clone()];
            // a piece may be looked at twice: after a lost End, the next piece starts over from the gap
            for _ in 0..2 {
                let outcome = match (&mut self.partial, p.kind) {
                    (None, MessageKind::Full) => {
                        if p.checksum_ok == Some(true) && p.tail_ok {
                            out.completed.push(Reassembled {
                                kind: p.user_kind,
                                bytes: payload.to_vec(),
                                parts: vec![part(&p.payload)],
                            });
                            PieceOutcome::Message
                        } else if !p.tail_ok {
                            out.pieces.push(info(&p, PieceOutcome::Dropped("bad tail")));
                            out.error = Some((p.head.start, "bad tail".into()));
                            break 'pieces;
                        } else {
                            PieceOutcome::Dropped("checksum mismatch")
                        }
                    }
                    (None, MessageKind::Start) => {
                        self.partial = Some(Partial {
                            kind: p.user_kind,
                            total: p.len,
                            bytes: payload.to_vec(),
                            parts: vec![part(&p.payload)],
                        });
                        PieceOutcome::Part
                    }
                    (None, MessageKind::Continue) => {
                        out.pieces
                            .push(info(&p, PieceOutcome::Dropped("its Start was lost")));
                        break 'pieces;
                    }
                    (None, MessageKind::End) => {
                        if p.tail_ok {
                            PieceOutcome::Dropped("its Start was lost")
                        } else {
                            out.pieces.push(info(&p, PieceOutcome::Dropped("bad tail")));
                            break 'pieces;
                        }
                    }
                    (Some(partial), MessageKind::Continue) => {
                        let remaining = partial.total - partial.bytes.len();
                        if partial.kind != p.user_kind || p.len != remaining {
                            self.drop_partial(&mut out);
                            out.pieces.push(info(
                                &p,
                                PieceOutcome::Dropped(
                                    "belongs to another message, a frame was lost",
                                ),
                            ));
                            break 'pieces;
                        }
                        partial.bytes.extend_from_slice(payload);
                        partial.parts.push(part(&p.payload));
                        PieceOutcome::Part
                    }
                    (Some(partial), MessageKind::End) => {
                        let remaining = partial.total - partial.bytes.len();
                        if partial.kind != p.user_kind || p.len != remaining {
                            // drop what was assembled, look at this End again from the gap
                            self.drop_partial(&mut out);
                            continue;
                        }
                        let mut partial = self.partial.take().expect("matched");
                        partial.bytes.extend_from_slice(payload);
                        partial.parts.push(part(&p.payload));
                        if !p.tail_ok {
                            out.dropped_partial = Some(partial.bytes.len());
                            out.pieces.push(info(&p, PieceOutcome::Dropped("bad tail")));
                            break 'pieces;
                        }
                        if self.walker.verify_split(
                            &frame[out.prefix.clone()],
                            &frame[p.head.clone()],
                            &partial.bytes,
                            &frame[p.trailer.clone()],
                        ) {
                            out.completed.push(Reassembled {
                                kind: partial.kind,
                                bytes: partial.bytes,
                                parts: partial.parts,
                            });
                            PieceOutcome::Message
                        } else {
                            out.dropped_partial = Some(partial.bytes.len());
                            PieceOutcome::Dropped(
                                "checksum mismatch, a frame was lost or reordered",
                            )
                        }
                    }
                    (Some(_), MessageKind::Full | MessageKind::Start) => {
                        // the End of the message being assembled was lost: drop it, start over from the gap
                        self.drop_partial(&mut out);
                        continue;
                    }
                };
                out.pieces.push(info(&p, outcome));
                break;
            }
        }
        out
    }

    fn drop_partial(&mut self, out: &mut FrameOutcome) {
        if let Some(p) = self.partial.take() {
            out.dropped_partial = Some(p.bytes.len());
        }
    }

    /// Stream media: chunks are cut anywhere; only Full messages, which may span chunks.
    fn feed_stream(&mut self, frame_no: u64, chunk: &[u8]) -> FrameOutcome {
        let mut out = FrameOutcome::default();
        let base = self.carry.len();
        self.carry.extend_from_slice(chunk);
        self.carry_from.push((frame_no, base, chunk.len(), 0));
        let mut consumed = 0;
        for p in self.walker.pieces(&self.carry) {
            match p {
                Ok(p) => {
                    consumed = p.trailer.end;
                    let outcome = if p.kind == MessageKind::Full && p.checksum_ok == Some(true) {
                        out.completed.push(Reassembled {
                            kind: p.user_kind,
                            bytes: self.carry[p.payload.clone()].to_vec(),
                            parts: self.parts_of(p.payload.clone()),
                        });
                        PieceOutcome::Message
                    } else {
                        PieceOutcome::Dropped("not a whole message")
                    };
                    let mut i = info(&p, outcome);
                    // ranges relative to this chunk where they fall into it
                    for r in [&mut i.head, &mut i.payload, &mut i.trailer] {
                        *r = r.start.saturating_sub(base)..r.end.saturating_sub(base);
                    }
                    out.pieces.push(i);
                }
                Err(e) => {
                    let wait = matches!(
                        e.kind,
                        PieceErrorKind::BadHead(RdError::NeedMoreData)
                            | PieceErrorKind::Truncated { .. }
                    );
                    if wait && self.carry.len() - e.at < 1 << 20 {
                        consumed = e.at;
                    } else {
                        out.error = Some((e.at.saturating_sub(base), describe(&e)));
                        consumed = self.carry.len();
                    }
                    break;
                }
            }
        }
        self.carry.drain(..consumed);
        // keep the chunk runs the remaining bytes are in, rebased
        let mut runs = Vec::new();
        for &(no, start, len, chunk_off) in &self.carry_from {
            let end = start + len;
            if end > consumed {
                let s = start.max(consumed);
                runs.push((no, s - consumed, end - s, chunk_off + (s - start)));
            }
        }
        self.carry_from = runs;
        out
    }

    /// Chunk parts of a range of the carry buffer.
    fn parts_of(&self, range: Range<usize>) -> Vec<FramePart> {
        let mut parts = Vec::new();
        for &(no, start, len, chunk_off) in &self.carry_from {
            let s = range.start.max(start);
            let e = range.end.min(start + len);
            if s < e {
                parts.push(FramePart {
                    frame_no: no,
                    offset: (chunk_off + s - start) as u32,
                    len: (e - s) as u32,
                });
            }
        }
        parts
    }
}

fn info(p: &Piece<u8>, outcome: PieceOutcome) -> PieceInfo {
    PieceInfo {
        kind: p.kind,
        user_kind: p.user_kind,
        len: p.len,
        head: p.head.clone(),
        payload: p.payload.clone(),
        trailer: p.trailer.clone(),
        outcome,
    }
}

fn describe(e: &PieceError) -> String {
    match e.kind {
        PieceErrorKind::BadHead(r) => format!("bad head ({r:?})"),
        PieceErrorKind::Truncated { needed, left } => {
            format!("piece runs past the frame: {needed} bytes needed, {left} left")
        }
        PieceErrorKind::BadSplit => "split piece without room for its End".into(),
        PieceErrorKind::TooShort => "frame shorter than its prefix".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Frames written with a `ww_link` framing decode whole through the tap's description of it, every message
    /// checked (CRC-32 on UDP); a corrupted byte in a UDP frame is caught.
    fn decodes_what_framing_writes<F: ww_link::Framing>(framer: Framer) {
        let (w, stream) = walker(&framer).unwrap().unwrap();
        assert!(!stream);
        let mut r = Reassembler::new(w, false);
        let mut tx = ww_link::FramingTxOwned::<F>::new(64);
        let messages: Vec<Vec<u8>> = (0..12u8).map(|i| vec![i; 1 + 13 * i as usize]).collect();
        let mut frames = vec![];
        for m in &messages {
            while !tx.write(0, m).unwrap() {
                frames.push(tx.flush_to_vec().unwrap());
            }
        }
        frames.push(tx.flush_to_vec().unwrap());
        let mut got = vec![];
        for (no, f) in frames.iter().enumerate() {
            let out = r.feed(no as u64, f);
            assert!(
                out.error.is_none() && out.dropped_partial.is_none(),
                "{out:?}"
            );
            got.extend(out.completed.into_iter().map(|m| m.bytes));
        }
        assert_eq!(got, messages);
    }

    #[test]
    fn tap_framers_match_ww_link() {
        decodes_what_framing_writes::<ww_link::UsbFraming>(crate::tap::FRAMED_USB);
        decodes_what_framing_writes::<ww_link::UsbSeqFraming>(crate::tap::FRAMED_USB_SEQ);
        decodes_what_framing_writes::<ww_link::UdpFraming>(crate::tap::FRAMED_UDP);
    }

    #[test]
    fn udp_crc_catches_a_flipped_bit() {
        let (w, _) = walker(&crate::tap::FRAMED_UDP).unwrap().unwrap();
        let mut r = Reassembler::new(w, false);
        let mut tx = ww_link::FramingTxOwned::<ww_link::UdpFraming>::new(64);
        assert!(tx.write(0, &[1, 2, 3]).unwrap());
        let mut frame = tx.flush_to_vec().unwrap();
        frame[3] ^= 0x10;
        let out = r.feed(0, &frame);
        assert!(out.completed.is_empty());
        assert_eq!(
            out.pieces[0].outcome,
            PieceOutcome::Dropped("checksum mismatch")
        );
    }

    /// A stream medium cuts chunks anywhere: messages spanning chunks come out whole, with the part of every chunk.
    #[test]
    fn stream_chunks_cut_anywhere() {
        let framer = Framer::U2 {
            checksum: FramerChecksum::None,
            tail: FramerTail::None,
            stream: true,
            prefix: FramerPrefix::None,
        };
        let (w, stream) = walker(&framer).unwrap().unwrap();
        assert!(stream);
        let mut r = Reassembler::new(w, true);
        let mut tx = ww_framer::TxOwned::<U2Head, NopChecksum, NopTail>::new(1024);
        let messages: Vec<Vec<u8>> = (0..5u8).map(|i| vec![i; 3 + 7 * i as usize]).collect();
        for m in &messages {
            assert!(tx.write_full(0, m).unwrap());
        }
        let bytes = tx.flush_to_vec().unwrap();
        let mut got = vec![];
        for (no, chunk) in bytes.chunks(5).enumerate() {
            got.extend(r.feed(no as u64, chunk).completed);
        }
        assert_eq!(got.len(), messages.len());
        let mut at = 0usize;
        for (g, m) in got.iter().zip(&messages) {
            assert_eq!(&g.bytes, m);
            // the head is one byte up to 7 bytes, two after
            at += if m.len() <= 7 { 1 } else { 2 };
            let first = g.parts[0];
            assert_eq!(first.frame_no as usize * 5 + first.offset as usize, at);
            let total: u32 = g.parts.iter().map(|p| p.len).sum();
            assert_eq!(total as usize, m.len());
            at += m.len();
        }
    }
}

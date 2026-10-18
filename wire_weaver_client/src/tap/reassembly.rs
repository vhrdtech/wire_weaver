//! Frames back into messages from their pieces ([ww_framer::pieces]), with the same rules `FramedRx` applies, and
//! for every message the frame bytes it was built from.

use std::marker::PhantomData;
use std::ops::Range;

use ww_framer::crc::{Crc16IbmSdlc, CrcChecksum};
use ww_framer::framed::U2Head;
use ww_framer::pieces::{Piece, PieceError, PieceErrorKind, pieces, verify_split};
use ww_framer::traits::{Checksum, MessageKind, NopChecksum, NopTail, RdError, Tail};
use ww_tap::{Framer, FramerChecksum, FramerTail};

/// The framer a session names, as code.
pub(crate) trait Walker: Send {
    fn pieces(&self, frame: &[u8]) -> Vec<Result<Piece<u8>, PieceError>>;
    fn verify_split(&self, message: &[u8], trailer: &[u8]) -> bool;
}

struct U2<C, T>(PhantomData<fn() -> (C, T)>);

impl<C: Checksum, T: Tail> Walker for U2<C, T> {
    fn pieces(&self, frame: &[u8]) -> Vec<Result<Piece<u8>, PieceError>> {
        pieces::<U2Head, C, T>(frame).collect()
    }

    fn verify_split(&self, message: &[u8], trailer: &[u8]) -> bool {
        verify_split::<C>(message, trailer)
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
    } = framer
    else {
        return Ok(None);
    };
    if !matches!(tail, FramerTail::None) {
        return Err(format!(
            "framer tail {tail:?} is not supported by this decoder"
        ));
    }
    let w: Box<dyn Walker> = match checksum {
        FramerChecksum::None => Box::new(U2::<NopChecksum, NopTail>(PhantomData)),
        FramerChecksum::Crc16IbmSdlc { full: false } => {
            Box::new(U2::<CrcChecksum<Crc16IbmSdlc, false>, NopTail>(PhantomData))
        }
        FramerChecksum::Crc16IbmSdlc { full: true } => {
            Box::new(U2::<CrcChecksum<Crc16IbmSdlc, true>, NopTail>(PhantomData))
        }
        other => {
            return Err(format!(
                "framer checksum {other:?} is not supported by this decoder"
            ));
        }
    };
    Ok(Some((w, *stream)))
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
                        if self
                            .walker
                            .verify_split(&partial.bytes, &frame[p.trailer.clone()])
                        {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stream medium cuts chunks anywhere: messages spanning chunks come out whole, with the part of every chunk.
    #[test]
    fn stream_chunks_cut_anywhere() {
        let framer = Framer::U2 {
            checksum: FramerChecksum::None,
            tail: FramerTail::None,
            stream: true,
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

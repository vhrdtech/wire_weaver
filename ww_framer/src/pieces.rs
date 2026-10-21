//! Walk one frame into its pieces, without reassembling anything: what a tap consumer needs to tell which messages
//! a frame carries and which frames a message was built from (`ww_tap`), offline and in both directions.
//!
//! [Pieces] reads heads with the same `read_head` and the same piece extents (`Extent`) that [FramedRx](crate::FramedRx)
//! uses, so the two can't drift. A consumer runs its own reassembly over the pieces (Start, Continue..., End) and
//! checks a split message's checksum on the whole message with [verify_split].

use core::marker::PhantomData;
use core::ops::Range;

use shrink_wrap::BufReader;

use crate::prefix::{FramePrefix, NopPrefix};
use crate::traits::{Checksum, Head, Header, MessageKind, RdError, Tail};

/// A head read at the start of `bytes`.
pub(crate) struct ReadHead<U> {
    pub kind: MessageKind,
    pub user_kind: U,
    /// The head's length field.
    pub len: usize,
    /// Bytes the head takes, byte aligned.
    pub head_len: usize,
    /// Bytes after the head.
    pub left: usize,
}

#[inline(always)]
pub(crate) fn read_head<H: Head>(bytes: &[u8]) -> Result<ReadHead<H::UserKind>, RdError> {
    let mut rd = BufReader::new(bytes);
    let (kind, user_kind, len) = H::read(&mut rd)?;
    rd.align_byte();
    Ok(ReadHead {
        kind,
        user_kind,
        len,
        head_len: rd.pos().0,
        left: rd.bytes_left(),
    })
}

/// Where the payload of a piece ends and what follows it, from its head alone.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Extent {
    /// Full or End: `payload` bytes, then the checksum and the tail (`trailer` bytes in total).
    Bounded { payload: usize, trailer: usize },
    /// Start or Continue: the payload extends till the end of the frame, no checksum or tail.
    TillEnd,
}

impl Extent {
    /// Payload, checksum and tail bytes of a Full or End; 0 for Start and Continue, whose length is the frame's.
    #[inline(always)]
    pub(crate) fn rest(self) -> usize {
        match self {
            Extent::Bounded { payload, trailer } => payload + trailer,
            Extent::TillEnd => 0,
        }
    }

    #[inline(always)]
    pub(crate) fn of<C: Checksum, T: Tail>(kind: MessageKind, len: usize) -> Self {
        match kind {
            MessageKind::Full => Extent::Bounded {
                payload: len,
                trailer: C::LEN_BYTES_FULL + T::LEN_BYTES,
            },
            MessageKind::End => Extent::Bounded {
                payload: len,
                trailer: C::LEN_BYTES_SPLIT + T::LEN_BYTES,
            },
            MessageKind::Start | MessageKind::Continue => Extent::TillEnd,
        }
    }
}

/// One piece of a frame: a whole message or a part of a split one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece<U> {
    pub kind: MessageKind,
    pub user_kind: U,
    /// The length field of the head: the whole message's length for Full and Start, the remaining length for
    /// Continue and End.
    pub len: usize,
    /// The head's bytes in the frame.
    pub head: Range<usize>,
    /// The message bytes this piece carries.
    pub payload: Range<usize>,
    /// Checksum and tail bytes after the payload (Full and End only, empty otherwise).
    pub trailer: Range<usize>,
    /// For Full: whether the checksum matched (true when there is none). None for split pieces: the checksum covers
    /// the whole message, see [verify_split].
    pub checksum_ok: Option<bool>,
    /// Whether the tail is what it should be (true for Start and Continue, which have none).
    pub tail_ok: bool,
}

/// Why the rest of a frame can't be walked; the frame's remaining bytes from `at` on are skipped, as
/// [FramedRx](crate::FramedRx) does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PieceError {
    /// Offset in the frame where the bad piece starts.
    pub at: usize,
    pub kind: PieceErrorKind,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PieceErrorKind {
    /// The head can't be read: not enough bytes for it (a stream medium's frame cut there), or a bad length form.
    BadHead(RdError),
    /// A Full or End whose payload, checksum and tail run past the end of the frame.
    Truncated { needed: usize, left: usize },
    /// A Start or Continue with no payload byte, or with at least the whole remaining length: a writer never makes
    /// those, it would have written a Full or an End.
    BadSplit,
    /// The frame is shorter than its [prefix](crate::prefix).
    TooShort,
}

/// Iterator over the pieces of one frame, see the [module docs](self). Ends after the last piece or after the first
/// error.
pub struct Pieces<'f, H, C, T, P = NopPrefix> {
    frame: &'f [u8],
    pos: usize,
    done: bool,
    _phantom: PhantomData<(H, C, T, P)>,
}

/// The pieces of `frame` without a prefix, see [Pieces].
pub fn pieces<H: Head, C: Checksum, T: Tail>(frame: &[u8]) -> Pieces<'_, H, C, T> {
    pieces_with_prefix::<H, C, T, NopPrefix>(frame)
}

/// The pieces of `frame`, which starts with a [prefix](crate::prefix) `P`, see [Pieces]. The prefix is not checked
/// against anything (that needs the frames before), only skipped and used for the checksums; [Pieces::prefix] has
/// its bytes.
pub fn pieces_with_prefix<H: Head, C: Checksum, T: Tail, P: FramePrefix>(
    frame: &[u8],
) -> Pieces<'_, H, C, T, P> {
    Pieces {
        frame,
        pos: P::LEN,
        done: false,
        _phantom: PhantomData,
    }
}

impl<H, C, T, P: FramePrefix> Pieces<'_, H, C, T, P> {
    /// The frame's prefix bytes, empty if it is too short for one.
    pub fn prefix(&self) -> &[u8] {
        self.frame.get(..P::LEN).unwrap_or(&[])
    }
}

impl<H: Head, C: Checksum, T: Tail, P: FramePrefix> Iterator for Pieces<'_, H, C, T, P> {
    type Item = Result<Piece<H::UserKind>, PieceError>;

    fn next(&mut self) -> Option<Self::Item> {
        if !self.done && self.frame.len() < P::LEN {
            self.done = true;
            return Some(Err(PieceError {
                at: 0,
                kind: PieceErrorKind::TooShort,
            }));
        }
        if self.done || self.pos >= self.frame.len() {
            return None;
        }
        let at = self.pos;
        let err = |kind| Some(Err(PieceError { at, kind }));
        let ReadHead {
            kind,
            user_kind,
            len,
            head_len,
            left,
        } = match read_head::<H>(&self.frame[at..]) {
            Ok(h) => h,
            Err(e) => {
                self.done = true;
                return err(PieceErrorKind::BadHead(e));
            }
        };
        let payload_start = at + head_len;
        match Extent::of::<C, T>(kind, len) {
            Extent::Bounded { payload, trailer } => {
                if payload + trailer > left {
                    self.done = true;
                    return err(PieceErrorKind::Truncated {
                        needed: payload + trailer,
                        left,
                    });
                }
                let payload_end = payload_start + payload;
                let trailer_end = payload_end + trailer;
                let message = &self.frame[payload_start..payload_end];
                let mut rd = BufReader::new(&self.frame[payload_end..trailer_end]);
                let checksum_ok = if kind == MessageKind::Full {
                    let header = Header::new(
                        C::LEN_BYTES_FULL > 0,
                        &self.frame[..P::LEN],
                        &self.frame[at..payload_start],
                    );
                    Some(C::read(header.get(), message, false, &mut rd).is_ok())
                } else {
                    // skip the checksum bytes, the tail comes after them
                    let mut skip = C::LEN_BYTES_SPLIT;
                    while skip > 0 && rd.read_u8().is_ok() {
                        skip -= 1;
                    }
                    None
                };
                let tail_ok = T::read(&mut rd).is_ok();
                self.pos = trailer_end;
                Some(Ok(Piece {
                    kind,
                    user_kind,
                    len,
                    head: at..payload_start,
                    payload: payload_start..payload_end,
                    trailer: payload_end..trailer_end,
                    checksum_ok,
                    tail_ok,
                }))
            }
            Extent::TillEnd => {
                if left == 0 || left >= len {
                    self.done = true;
                    return err(PieceErrorKind::BadSplit);
                }
                let end = self.frame.len();
                self.pos = end;
                Some(Ok(Piece {
                    kind,
                    user_kind,
                    len,
                    head: at..payload_start,
                    payload: payload_start..end,
                    trailer: end..end,
                    checksum_ok: None,
                    tail_ok: true,
                }))
            }
        }
    }
}

/// Check a reassembled split message against the trailer of its End piece ([Piece::trailer]); `prefix` is the
/// prefix of the End's frame ([Pieces::prefix], empty without one) and `end_head` the End's head bytes
/// ([Piece::head]), both covered by the checksum.
pub fn verify_split<C: Checksum>(
    prefix: &[u8],
    end_head: &[u8],
    message: &[u8],
    trailer: &[u8],
) -> bool {
    if end_head.len() > crate::traits::MAX_COVERED_HEAD || prefix.len() > crate::prefix::MAX_PREFIX_LEN {
        return false;
    }
    let header = Header::new(C::LEN_BYTES_SPLIT > 0, prefix, end_head);
    let mut rd = BufReader::new(trailer);
    C::read(header.get(), message, true, &mut rd).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FramedRx;
    use crate::Tx;
    use crate::framed::U2Head;
    use crate::tests::common::*;
    use crate::traits::{ByteTail, NopChecksum, NopTail};

    fn walk<C: Checksum, T: Tail>(frame: &[u8]) -> std::vec::Vec<Result<Piece<u8>, PieceError>> {
        pieces::<U2Head, C, T>(frame).collect()
    }

    extern crate std;

    #[test]
    fn several_full_and_a_start() {
        let frame = [FULL_4, 1, 2, 3, 4, FULL_0, START_4, 9, 8];
        let got = walk::<NopChecksum, NopTail>(&frame);
        assert_eq!(got.len(), 3);
        let p0 = got[0].as_ref().unwrap();
        assert_eq!(
            (p0.kind, p0.len, p0.head.clone(), p0.payload.clone()),
            (MessageKind::Full, 4, 0..1, 1..5)
        );
        assert_eq!(p0.checksum_ok, Some(true));
        let p1 = got[1].as_ref().unwrap();
        assert_eq!((p1.kind, p1.payload.clone()), (MessageKind::Full, 6..6));
        let p2 = got[2].as_ref().unwrap();
        assert_eq!(
            (p2.kind, p2.len, p2.payload.clone()),
            (MessageKind::Start, 4, 7..9)
        );
    }

    #[test]
    fn end_then_full() {
        let frame = [END_1, 4, FULL_1, 7];
        let got = walk::<NopChecksum, NopTail>(&frame);
        let kinds: std::vec::Vec<_> = got.iter().map(|p| p.as_ref().unwrap().kind).collect();
        assert_eq!(kinds, [MessageKind::End, MessageKind::Full]);
        assert_eq!(got[0].as_ref().unwrap().checksum_ok, None);
    }

    #[test]
    fn errors_stop_the_walk() {
        let got = walk::<NopChecksum, NopTail>(&[FULL_4, 1, 2]);
        assert_eq!(
            got,
            [Err(PieceError {
                at: 0,
                kind: PieceErrorKind::Truncated { needed: 4, left: 2 }
            })]
        );
        let got = walk::<NopChecksum, NopTail>(&[FULL_0, FULL_BAD_LEN, 1]);
        assert_eq!(got.len(), 2);
        assert!(matches!(
            got[1],
            Err(PieceError {
                at: 1,
                kind: PieceErrorKind::BadHead(_)
            })
        ));
        // a Start that holds the whole message
        let got = walk::<NopChecksum, NopTail>(&[START_4, 1, 2, 3, 4]);
        assert!(matches!(
            got[0],
            Err(PieceError {
                kind: PieceErrorKind::BadSplit,
                ..
            })
        ));
    }

    #[test]
    fn checksum_and_tail() {
        // Full: XOR of 1, 2 = 3, tail 0xEE
        let good = walk::<XorChecksum, ByteTail<0xEE>>(&[0b0000_0010, 1, 2, 3, 0xEE]);
        let p = good[0].as_ref().unwrap();
        assert_eq!((p.checksum_ok, p.tail_ok), (Some(true), true));
        assert_eq!(p.trailer, 3..5);
        let bad = walk::<XorChecksum, ByteTail<0xEE>>(&[0b0000_0010, 1, 2, 4, 0xEF]);
        let p = bad[0].as_ref().unwrap();
        assert_eq!((p.checksum_ok, p.tail_ok), (Some(false), false));
        // End: checksum over the whole message, checked by the consumer
        let end = walk::<XorChecksum, ByteTail<0xEE>>(&[END_1, 3, 1 ^ 2 ^ 3, 0xEE]);
        let p = end[0].as_ref().unwrap();
        assert_eq!((p.checksum_ok, p.tail_ok), (None, true));
        assert!(verify_split::<XorChecksum>(&[], &[END_1], &[1, 2, 3], &[1 ^ 2 ^ 3, 0xEE]));
        assert!(!verify_split::<XorChecksum>(&[], &[END_1], &[1, 2, 4], &[1 ^ 2 ^ 3, 0xEE]));
    }

    /// Pieces reassembled by hand give the same messages as [FramedRx], over frames a [Tx] wrote from messages of
    /// every length around the frame size.
    #[test]
    fn same_messages_as_framed_rx() {
        type C = XorChecksum;
        type T = ByteTail<0xEE>;
        for frame_size in [4usize, 5, 8, 13, 64] {
            let mut messages: std::vec::Vec<(u8, std::vec::Vec<u8>)> = std::vec::Vec::new();
            for len in 0..(3 * frame_size) {
                messages.push((
                    (len % 3) as u8,
                    (0..len).map(|i| (i * 7 + len) as u8).collect(),
                ));
            }
            // write
            let mut frames: std::vec::Vec<std::vec::Vec<u8>> = std::vec::Vec::new();
            let mut buf = std::vec![0u8; frame_size];
            let mut tx = Tx::<U2Head, C, T>::new(&mut buf);
            for (uk, m) in &messages {
                while !tx.write(*uk, m).expect("fits") {
                    let n = tx.flush();
                    assert!(n > 0);
                    frames.push(tx.buf()[..n].to_vec());
                }
            }
            let n = tx.flush();
            if n > 0 {
                frames.push(tx.buf()[..n].to_vec());
            }

            // FramedRx
            let mut rx_buf = std::vec![0u8; 4 * frame_size + 64];
            let mut rx = FramedRx::<U2Head, C, T>::new(&mut rx_buf);
            let mut by_rx = std::vec::Vec::new();
            for f in &frames {
                rx.stage(f).unwrap();
                loop {
                    rx.reassemble();
                    match rx.message() {
                        Some((uk, m)) => by_rx.push((uk, m.to_vec())),
                        None => break,
                    }
                }
            }

            // pieces
            let mut by_pieces = std::vec::Vec::new();
            let mut assembling: Option<(u8, std::vec::Vec<u8>)> = None;
            for f in &frames {
                for p in pieces::<U2Head, C, T>(f) {
                    let p = p.unwrap();
                    let bytes = &f[p.payload.clone()];
                    match p.kind {
                        MessageKind::Full => {
                            assert_eq!(p.checksum_ok, Some(true));
                            by_pieces.push((p.user_kind, bytes.to_vec()));
                        }
                        MessageKind::Start => assembling = Some((p.user_kind, bytes.to_vec())),
                        MessageKind::Continue => assembling.as_mut().unwrap().1.extend(bytes),
                        MessageKind::End => {
                            let (uk, mut m) = assembling.take().unwrap();
                            m.extend(bytes);
                            assert!(verify_split::<C>(&[], &f[p.head.clone()], &m, &f[p.trailer.clone()]));
                            by_pieces.push((uk, m));
                        }
                    }
                }
            }
            assert_eq!(by_rx, messages, "frame size {frame_size}");
            assert_eq!(by_pieces, messages, "frame size {frame_size}");
        }
    }
}

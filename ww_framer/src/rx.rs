use core::marker::PhantomData;

use shrink_wrap::BufReader;

use crate::traits::{Checksum, Head, MessageKind, RdError, Tail};

pub struct Rx<'i, H: Head, C, T> {
    /// Holds up two one maximum Message size + one maximum input packet size (e.g., USB packet)
    ///
    /// For example if assembly_buf is 1024B, and maximum re-assembled message is 512, it could be
    /// that message is almost ready (e.g., 511B) and a whole new packet comes.
    /// Immediately a message is ready and must be returned with the approach used.
    /// But remaining bytes from the packet must be stored somewhere in the mean-time as well.
    ///
    /// Layout: `[assembled message bytes][processed frame bytes][unprocessed frame bytes][free]`
    /// Assembled bytes are held at `assembly_buf[..assembled]` (see [State::Assembling]),
    /// staged frame at `assembly_buf[staging_pos..staging_end]`.
    assembly_buf: &'i mut [u8],

    /// Index of the next unprocessed staged byte.
    staging_pos: usize,
    /// Index one past the last staged byte. Data after this is free.
    staging_end: usize,

    state: State<H::UserKind>,

    _phantom_c: PhantomData<C>,
    _phantom_t: PhantomData<T>,
}

enum State<U> {
    /// No message is being assembled, nothing is ready.
    Gap,
    /// A split message is being assembled over several frames.
    /// Bytes received so far are held at `assembly_buf[..assembled]`.
    Assembling {
        user_kind: U,
        assembled: usize,
        total: usize,
    },
    /// A message is ready and held at `assembly_buf[start..start + len]`.
    /// `start == 0` for re-assembled messages, in-place inside staged frame for Full messages.
    Ready {
        user_kind: U,
        start: usize,
        len: usize,
    },
}

impl<'b, 'i: 'b, H: Head, C: Checksum, T: Tail> Rx<'i, H, C, T>
where
    H::UserKind: Copy + PartialEq,
{
    /// Create new framer from the provided assembly buffer.
    /// Buffer must hold at least one maximum re-assembled message + one maximum frame.
    pub fn new(assembly_buf: &'i mut [u8]) -> Self {
        // to catch obviously way too small buffers, can't know user message size here
        debug_assert!(assembly_buf.len() >= H::MIN_FRAME_SIZE);
        Rx {
            assembly_buf,
            staging_pos: 0,
            staging_end: 0,
            state: State::Gap,
            _phantom_c: PhantomData,
            _phantom_t: PhantomData,
        }
    }

    /// Returns the number of bytes that can be staged
    pub fn free(&self) -> usize {
        self.assembly_buf.len() - self.staging_end
    }

    /// Call with a next received frame, then call [Self::reassemble] and
    /// [Self::message] in a loop until getting None.
    ///
    /// Frame boundaries are significant (Start / Continue messages extend till the end of a frame),
    /// so for framed media with split messages, [Self::reassemble] must be called after each frame.
    /// Bytes of an incomplete message (e.g. head or checksum not yet fully received) are kept
    /// and the next staged frame is appended to them.
    ///
    /// Returns Err(()) if there is not enough space.
    pub fn stage(&mut self, frame: &[u8]) -> Result<(), ()> {
        if frame.len() > self.free() {
            return Err(());
        }
        self.assembly_buf[self.staging_end..self.staging_end + frame.len()].copy_from_slice(frame);
        self.staging_end += frame.len();
        Ok(())
    }

    /// Process staged bytes until a message is ready, staged frame is exhausted or more data is
    /// needed to complete a message (returns cleanly, retry after staging more).
    /// Discards previously ready message (if any).
    ///
    /// On any error, remaining bytes of the current frame are skipped at once.
    pub fn reassemble(&mut self) {
        // consume previously returned message
        if let State::Ready { .. } = self.state {
            self.state = State::Gap;
        }
        loop {
            if self.staging_pos >= self.staging_end {
                self.compact();
                return;
            }
            match self.process_next() {
                Step::Ready => return,
                Step::Next => continue,
                Step::SkipFrame => {
                    self.staging_pos = self.staging_end;
                    self.compact();
                    return;
                }
                Step::NeedMoreData => {
                    self.compact_partial();
                    return;
                }
            }
        }
    }

    /// Intented use:
    /// ```ignore
    /// rx.stage(frame)?;
    /// loop {
    ///     rx.reassemble();
    ///     let Some((kind, message)) = self.message() else {
    ///         break;
    ///     }
    /// }
    /// ```
    pub fn message(&self) -> Option<(H::UserKind, &[u8])> {
        match self.state {
            State::Ready {
                user_kind,
                start,
                len,
            } => Some((user_kind, &self.assembly_buf[start..start + len])),
            _ => None,
        }
    }

    /// Reset staging area to right after assembled bytes, when frame is fully processed.
    fn compact(&mut self) {
        let base = match self.state {
            State::Gap => 0,
            State::Assembling { assembled, .. } => assembled,
            // keep in-place message intact, compaction happens once it is consumed
            State::Ready { .. } => return,
        };
        self.staging_pos = base;
        self.staging_end = base;
    }

    /// Move incomplete message bytes down to right after assembled bytes, so that as much space
    /// as possible is available for the next frame.
    fn compact_partial(&mut self) {
        let base = match self.state {
            State::Gap => 0,
            State::Assembling { assembled, .. } => assembled,
            State::Ready { .. } => return,
        };
        if self.staging_pos > base {
            self.assembly_buf
                .copy_within(self.staging_pos..self.staging_end, base);
            self.staging_end -= self.staging_pos - base;
            self.staging_pos = base;
        }
    }

    /// Whether a message can fit into the buffer at all, once everything is compacted:
    /// `[assembled][head][payload + checksum + tail]`
    fn can_ever_fit(&self, assembled: usize, head_len: usize, rest: usize) -> bool {
        assembled + head_len + rest <= self.assembly_buf.len()
    }

    fn process_next(&mut self) -> Step {
        let mut rd = BufReader::new(&self.assembly_buf[self.staging_pos..self.staging_end]);
        let (kind, user_kind, len) = match H::read(&mut rd) {
            Ok(h) => h,
            Err(RdError::NeedMoreData) => return Step::NeedMoreData,
            Err(_) => return Step::SkipFrame,
        };
        rd.align_byte();
        let payload_start = self.staging_pos + rd.pos().0;
        let head_len = payload_start - self.staging_pos;
        let frame_left = rd.bytes_left();

        match (&self.state, kind) {
            (State::Gap, MessageKind::Full) => {
                let rest = len + C::LEN_BYTES_FULL + T::LEN_BYTES;
                if rest > frame_left {
                    return if self.can_ever_fit(0, head_len, rest) {
                        Step::NeedMoreData
                    } else {
                        Step::SkipFrame
                    };
                }
                let message = &self.assembly_buf[payload_start..payload_start + len];
                let mut rd =
                    BufReader::new(&self.assembly_buf[payload_start + len..self.staging_end]);
                match C::read(message, false, &mut rd).and_then(|_| T::read(&mut rd)) {
                    Ok(_) => {}
                    Err(RdError::NeedMoreData) => return Step::NeedMoreData,
                    Err(_) => return Step::SkipFrame,
                }
                rd.align_byte();
                self.staging_pos = payload_start + len + rd.pos().0;
                self.state = State::Ready {
                    user_kind,
                    start: payload_start,
                    len,
                };
                Step::Ready
            }
            (State::Gap, MessageKind::Start) => {
                // Start extends till the end of the frame
                if len > self.assembly_buf.len() || frame_left == 0 || frame_left >= len {
                    return Step::SkipFrame;
                }
                self.assembly_buf
                    .copy_within(payload_start..payload_start + frame_left, 0);
                self.staging_pos = self.staging_end;
                self.state = State::Assembling {
                    user_kind,
                    assembled: frame_left,
                    total: len,
                };
                Step::Next
            }
            (State::Gap, MessageKind::Continue | MessageKind::End) => {
                // lost Start, skip
                Step::SkipFrame
            }
            (
                &State::Assembling {
                    user_kind: uk,
                    assembled,
                    total,
                },
                MessageKind::Continue,
            ) => {
                // Continue extends till the end of the frame
                let remaining = total - assembled;
                if uk != user_kind || frame_left == 0 || frame_left >= remaining {
                    self.state = State::Gap;
                    return Step::SkipFrame;
                }
                self.assembly_buf
                    .copy_within(payload_start..payload_start + frame_left, assembled);
                self.staging_pos = self.staging_end;
                self.state = State::Assembling {
                    user_kind,
                    assembled: assembled + frame_left,
                    total,
                };
                Step::Next
            }
            (
                &State::Assembling {
                    user_kind: uk,
                    assembled,
                    total,
                },
                MessageKind::End,
            ) => {
                let remaining = total - assembled;
                if uk != user_kind {
                    self.state = State::Gap;
                    return Step::SkipFrame;
                }
                let rest = remaining + C::LEN_BYTES_SPLIT + T::LEN_BYTES;
                if rest > frame_left {
                    return if self.can_ever_fit(assembled, head_len, rest) {
                        Step::NeedMoreData
                    } else {
                        self.state = State::Gap;
                        Step::SkipFrame
                    };
                }
                self.assembly_buf
                    .copy_within(payload_start..payload_start + remaining, assembled);
                let after_payload = payload_start + remaining;
                let message = &self.assembly_buf[..total];
                let mut rd = BufReader::new(&self.assembly_buf[after_payload..self.staging_end]);
                match C::read(message, true, &mut rd).and_then(|_| T::read(&mut rd)) {
                    Ok(_) => {}
                    Err(RdError::NeedMoreData) => return Step::NeedMoreData,
                    Err(_) => {
                        self.state = State::Gap;
                        return Step::SkipFrame;
                    }
                }
                rd.align_byte();
                self.staging_pos = after_payload + rd.pos().0;
                self.state = State::Ready {
                    user_kind,
                    start: 0,
                    len: total,
                };
                Step::Ready
            }
            (State::Assembling { .. }, MessageKind::Full | MessageKind::Start) => {
                // lost End of the previous message, drop it and re-process this one from Gap
                self.state = State::Gap;
                Step::Next
            }
            (State::Ready { .. }, _) => unreachable!("Ready is consumed at reassemble() start"),
        }
    }
}

enum Step {
    /// Message is ready, stop processing
    Ready,
    /// Continue with the next message in the frame
    Next,
    /// Error encountered, skip the rest of the frame
    SkipFrame,
    /// Message is incomplete, keep its bytes and wait for the next frame
    NeedMoreData,
}

#[cfg(test)]
mod tests {
    use super::Rx;
    use crate::Tx;
    use crate::framed::U2Head;
    use crate::traits::{NopChecksum, NopTail};

    type TestRx<'i> = Rx<'i, U2Head, NopChecksum, NopTail>;
    type TestTx<'i> = Tx<'i, U2Head, NopChecksum, NopTail>;

    /// Stage one frame and assert exactly `expected` messages come out of it.
    fn feed(rx: &mut TestRx<'_>, frame: &[u8], expected: &[(u8, &[u8])]) {
        rx.stage(frame).unwrap();
        for (i, (kind, msg)) in expected.iter().enumerate() {
            rx.reassemble();
            let got = rx.message();
            assert_eq!(
                got,
                Some((*kind, *msg)),
                "message #{i} of frame {frame:02x?}"
            );
        }
        rx.reassemble();
        assert_eq!(rx.message(), None, "extra message after frame {frame:02x?}");
    }

    // Head bytes (see U2Head docs): mm uu 0lll
    const FULL_EMPTY: u8 = 0b0000_0000;
    const FULL_4: u8 = 0b0000_0001;
    const START_4: u8 = 0b0100_0001;
    const START_7: u8 = 0b0100_0100;
    const CONTINUE: u8 = 0b1000_0000;
    const END: u8 = 0b1100_0000;
    const CONTINUE_UK1: u8 = 0b1001_0000;

    #[test]
    fn empty_and_extended_user_kind() {
        let mut buf = [0u8; 16];
        let mut rx = TestRx::new(&mut buf);
        feed(&mut rx, &[FULL_EMPTY], &[(0, &[])]);
        feed(&mut rx, &[0b0011_1111, 0b1111_0000], &[(255, &[])]);
        assert_eq!(rx.free(), 16);
    }

    #[test]
    fn full_message_in_place() {
        let mut buf = [0u8; 16];
        let mut rx = TestRx::new(&mut buf);
        feed(&mut rx, &[FULL_4, 1, 2, 3, 4], &[(0, &[1, 2, 3, 4])]);
    }

    #[test]
    fn several_full_messages_in_one_frame() {
        let mut buf = [0u8; 16];
        let mut rx = TestRx::new(&mut buf);
        feed(
            &mut rx,
            &[FULL_4, 1, 2, 3, 4, FULL_EMPTY, 0b0001_0001, 5, 6, 7, 8],
            &[(0, &[1, 2, 3, 4]), (0, &[]), (1, &[5, 6, 7, 8])],
        );
    }

    #[test]
    fn split_start_end() {
        let mut buf = [0u8; 16];
        let mut rx = TestRx::new(&mut buf);
        feed(&mut rx, &[START_4, 0xAA, 0xBB, 0xCC], &[]);
        feed(&mut rx, &[END, 0xDD], &[(0, &[0xAA, 0xBB, 0xCC, 0xDD])]);
    }

    #[test]
    fn split_with_continue() {
        let mut buf = [0u8; 16];
        let mut rx = TestRx::new(&mut buf);
        feed(&mut rx, &[START_7, 1, 2, 3], &[]);
        feed(&mut rx, &[CONTINUE, 4, 5, 6], &[]);
        feed(&mut rx, &[END, 7], &[(0, &[1, 2, 3, 4, 5, 6, 7])]);
    }

    #[test]
    fn end_followed_by_full_in_same_frame() {
        let mut buf = [0u8; 16];
        let mut rx = TestRx::new(&mut buf);
        feed(&mut rx, &[START_4, 1, 2, 3], &[]);
        feed(
            &mut rx,
            &[END, 4, FULL_EMPTY, FULL_4, 5, 6, 7, 8],
            &[(0, &[1, 2, 3, 4]), (0, &[]), (0, &[5, 6, 7, 8])],
        );
    }

    #[test]
    fn full_followed_by_start_in_same_frame() {
        let mut buf = [0u8; 16];
        let mut rx = TestRx::new(&mut buf);
        feed(&mut rx, &[FULL_EMPTY, START_4, 1, 2], &[(0, &[])]);
        feed(&mut rx, &[END, 3, 4], &[(0, &[1, 2, 3, 4])]);
    }

    #[test]
    fn split_extended_user_kind() {
        let mut buf = [0u8; 16];
        let mut rx = TestRx::new(&mut buf);
        feed(&mut rx, &[0b0111_1111, 0b1111_1000, 0b0000_0011, 0xAA], &[]);
        feed(
            &mut rx,
            &[0b1111_1111, 0b1111_0000, 0xBB, 0xCC],
            &[(255, &[0xAA, 0xBB, 0xCC])],
        );
    }

    #[test]
    fn lost_start_skips_frame() {
        let mut buf = [0u8; 16];
        let mut rx = TestRx::new(&mut buf);
        feed(&mut rx, &[CONTINUE, 1, 2, 3], &[]);
        feed(&mut rx, &[END, 4], &[]);
        // link recovers afterwards
        feed(&mut rx, &[FULL_EMPTY], &[(0, &[])]);
    }

    #[test]
    fn lost_end_drops_partial_and_delivers_next() {
        let mut buf = [0u8; 16];
        let mut rx = TestRx::new(&mut buf);
        feed(&mut rx, &[START_4, 1, 2, 3], &[]);
        // End frame lost, a Full one arrives instead
        feed(&mut rx, &[FULL_4, 5, 6, 7, 8], &[(0, &[5, 6, 7, 8])]);
        // Start after lost End is re-processed as well
        feed(&mut rx, &[START_4, 1, 2, 3], &[]);
        feed(&mut rx, &[START_4, 9, 8, 7], &[]);
        feed(&mut rx, &[END, 6], &[(0, &[9, 8, 7, 6])]);
    }

    #[test]
    fn user_kind_mismatch_drops_message() {
        let mut buf = [0u8; 16];
        let mut rx = TestRx::new(&mut buf);
        feed(&mut rx, &[START_7, 1, 2, 3], &[]);
        feed(&mut rx, &[CONTINUE_UK1, 4, 5, 6], &[]);
        feed(&mut rx, &[END, 7], &[]);
        feed(&mut rx, &[FULL_EMPTY], &[(0, &[])]);
    }

    #[test]
    fn inconsistent_split_skips_frame() {
        let mut buf = [0u8; 16];
        let mut rx = TestRx::new(&mut buf);
        // Start with all bytes present (should have been Full)
        feed(&mut rx, &[START_4, 1, 2, 3, 4, 5], &[]);
        // Start with no payload
        feed(&mut rx, &[START_4], &[]);
        // Continue that would complete the message
        feed(&mut rx, &[START_4, 1, 2], &[]);
        feed(&mut rx, &[CONTINUE, 3, 4], &[]);
        feed(&mut rx, &[END, 4], &[]);
        assert_eq!(rx.free(), 16);
    }

    #[test]
    fn partial_full_waits_for_more_data() {
        let mut buf = [0u8; 16];
        let mut rx = TestRx::new(&mut buf);
        // payload split across chunks
        feed(&mut rx, &[FULL_EMPTY, FULL_4, 1, 2], &[(0, &[])]);
        assert_eq!(rx.free(), 13); // incomplete message compacted to the start
        feed(
            &mut rx,
            &[3, 4, FULL_EMPTY],
            &[(0, &[1, 2, 3, 4]), (0, &[])],
        );
        assert_eq!(rx.free(), 16);

        // head split across chunks (extended user kind, 2 byte head)
        feed(&mut rx, &[0b0011_1111], &[]);
        feed(&mut rx, &[0b1111_0000], &[(255, &[])]);

        // byte by byte
        for b in [FULL_4, 1, 2, 3] {
            feed(&mut rx, &[b], &[]);
        }
        feed(&mut rx, &[4], &[(0, &[1, 2, 3, 4])]);
        assert_eq!(rx.free(), 16);
    }

    #[test]
    fn partial_end_waits_for_more_data() {
        let mut buf = [0u8; 16];
        let mut rx = TestRx::new(&mut buf);
        feed(&mut rx, &[START_7, 1, 2, 3], &[]);
        feed(&mut rx, &[END, 4, 5], &[]);
        assert_eq!(rx.free(), 10); // 3 assembled + [END, 4, 5] kept
        feed(&mut rx, &[6], &[]);
        feed(
            &mut rx,
            &[7, FULL_EMPTY],
            &[(0, &[1, 2, 3, 4, 5, 6, 7]), (0, &[])],
        );
        assert_eq!(rx.free(), 16);
    }

    #[test]
    fn full_that_can_never_fit_is_skipped() {
        let mut buf = [0u8; 8];
        let mut rx = TestRx::new(&mut buf);
        // Full, len = 11: 0b0000_1000, 0b0000_1011
        feed(&mut rx, &[0b0000_1000, 0b0000_1011, 1, 2], &[]);
        assert_eq!(rx.free(), 8);
        feed(&mut rx, &[FULL_EMPTY], &[(0, &[])]);
    }

    #[test]
    fn end_that_can_never_fit_is_skipped() {
        let mut buf = [0u8; 8];
        let mut rx = TestRx::new(&mut buf);
        // Start len = 8: assembled(3) + End head(1) + remaining(5) = 9 > 8
        feed(&mut rx, &[0b0100_0101, 1, 2, 3], &[]);
        feed(&mut rx, &[END, 4, 5], &[]);
        assert_eq!(rx.free(), 8);
        feed(&mut rx, &[FULL_EMPTY], &[(0, &[])]);
    }

    #[test]
    fn message_too_big_for_buffer() {
        let mut buf = [0u8; 8];
        let mut rx = TestRx::new(&mut buf);
        // Start, len = 11: 0b0100_1000, 0b0000_1011
        feed(&mut rx, &[0b0100_1000, 0b0000_1011, 1, 2], &[]);
        feed(&mut rx, &[END, 3], &[]);
        assert_eq!(rx.free(), 8);
    }

    #[test]
    fn stage_errors() {
        let mut buf = [0u8; 8];
        let mut rx = TestRx::new(&mut buf);
        assert_eq!(rx.stage(&[0u8; 9]), Err(()));
        rx.stage(&[0u8; 8]).unwrap();
        assert_eq!(rx.free(), 0);
        assert_eq!(rx.stage(&[FULL_EMPTY]), Err(()));
        for _ in 0..8 {
            rx.reassemble();
            assert_eq!(rx.message(), Some((0, &[][..])));
        }
        rx.reassemble();
        assert_eq!(rx.message(), None);
        // several Full messages can be staged before reassembling
        rx.stage(&[FULL_EMPTY, FULL_EMPTY]).unwrap();
        rx.stage(&[FULL_4, 1, 2, 3, 4]).unwrap();
        for _ in 0..2 {
            rx.reassemble();
            assert_eq!(rx.message(), Some((0, &[][..])));
        }
        rx.reassemble();
        assert_eq!(rx.message(), Some((0, &[1, 2, 3, 4][..])));
        rx.reassemble();
        assert_eq!(rx.message(), None);
        assert_eq!(rx.free(), 8);
    }

    #[test]
    fn free_accounts_for_assembled_bytes() {
        let mut buf = [0u8; 16];
        let mut rx = TestRx::new(&mut buf);
        feed(&mut rx, &[START_7, 1, 2, 3], &[]);
        assert_eq!(rx.free(), 13);
        feed(&mut rx, &[CONTINUE, 4, 5, 6], &[]);
        assert_eq!(rx.free(), 10);
        feed(&mut rx, &[END, 7], &[(0, &[1, 2, 3, 4, 5, 6, 7])]);
        assert_eq!(rx.free(), 16);
    }

    #[test]
    fn tx_rx_round_trip() {
        let messages: [(u8, &[u8]); 6] = [
            (0, &[]),
            (1, &[0xAA, 0xBB, 0xCC, 0xDD]),
            (255, &[1, 2, 3]),
            (2, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]),
            (0, &[0x55]),
            (3, &[9, 8, 7, 6, 5]),
        ];

        let mut tx_buf = [0u8; 4];
        let mut tx = TestTx::new(&mut tx_buf);
        let mut rx_buf = [0u8; 32];
        let mut rx = TestRx::new(&mut rx_buf);

        let mut received = 0;
        for (user_kind, message) in messages {
            loop {
                let done = tx.write(user_kind, message).unwrap();
                let len = tx.flush();
                rx.stage(&tx.buf()[..len]).unwrap();
                loop {
                    rx.reassemble();
                    let Some((k, m)) = rx.message() else { break };
                    assert_eq!((k, m), messages[received]);
                    received += 1;
                }
                if done {
                    break;
                }
            }
        }
        assert_eq!(received, messages.len());
    }
}

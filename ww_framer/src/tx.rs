use core::marker::PhantomData;

use shrink_wrap::{BufWriter, buf_writer::BufWriterState};

use crate::prefix::{FrameCtx, FramePrefix, MAX_PREFIX_LEN, NopPrefix};
use crate::traits::{Checksum, Head, Header, MAX_COVERED_HEAD, MessageKind, Tail, WrError};

/// Packs messages into frames, see the crate docs. `P` is the [frame prefix](crate::prefix) written at the start of
/// every frame, none by default.
pub struct Tx<'i, H, C, T, P = NopPrefix> {
    wr: BufWriter<'i>,
    state: State,
    frame: FrameCtx,
    _phantom_h: PhantomData<fn() -> H>,
    _phantom_c: PhantomData<fn() -> C>,
    _phantom_t: PhantomData<fn() -> T>,
    _phantom_p: PhantomData<fn() -> P>,
}

#[derive(Copy, Clone)]
enum State {
    Gap,
    WroteN(usize),
}

/// Everything [Tx] holds apart from the assembly buffer itself, see [Tx::into_parts].
///
/// Allows to temporarily give the buffer back to its owner and re-create [Tx] later with
/// [Tx::from_parts], e.g. to implement an owned variant without duplicating the logic.
#[derive(Copy, Clone)]
pub struct TxState {
    wr: BufWriterState,
    state: State,
    frame: FrameCtx,
}

impl TxState {
    /// Same as [Tx::is_empty], without re-creating [Tx].
    pub fn is_empty(&self) -> bool {
        self.wr.pos().0 == 0
    }
}

impl<'i, H: Head, C: Checksum, T: Tail, P: FramePrefix> Tx<'i, H, C, T, P>
where
    H::UserKind: Copy,
{
    /// Create new framer from the provided assembly buffer.
    /// Buffer must be exactly the length of the maximum frame (or DMA size).
    pub fn new(assembly_buf: &'i mut [u8]) -> Self {
        const { assert!(P::LEN <= MAX_PREFIX_LEN) };
        const { assert!(H::MAX_HEAD_SIZE <= MAX_COVERED_HEAD) };
        #[cfg(not(test))] // to catch potentially incorrect H::MIN_FRAME_SIZE in tests
        debug_assert!(assembly_buf.len() >= H::MIN_FRAME_SIZE + P::LEN);
        Tx {
            wr: BufWriter::new(assembly_buf),
            state: State::Gap,
            frame: FrameCtx::default(),
            _phantom_h: PhantomData,
            _phantom_c: PhantomData,
            _phantom_t: PhantomData,
            _phantom_p: PhantomData,
        }
    }

    /// Re-create framer from the same assembly buffer and state previously obtained from [Self::into_parts].
    pub fn from_parts(assembly_buf: &'i mut [u8], parts: TxState) -> Self {
        let mut wr = BufWriter::new(assembly_buf);
        wr.restore_state(parts.wr);
        Tx {
            wr,
            state: parts.state,
            frame: parts.frame,
            _phantom_h: PhantomData,
            _phantom_c: PhantomData,
            _phantom_t: PhantomData,
            _phantom_p: PhantomData,
        }
    }

    /// Release the assembly buffer, keeping all the state needed to continue later with [Self::from_parts].
    pub fn into_parts(self) -> TxState {
        TxState {
            wr: self.wr.save_state(),
            state: self.state,
            frame: self.frame,
        }
    }

    /// Drop the current frame and a partially written message (if any),
    /// e.g., after the medium was disconnected. The frame counter and epoch go on.
    pub fn reset(&mut self) {
        self.wr.reset();
        self.state = State::Gap;
    }

    /// Start a new session (e.g., on link setup): [Self::reset], the epoch goes up by one and frames are counted
    /// from 0 again, so that a receiver with a counting [prefix](crate::prefix) drops whatever is left of the
    /// previous session.
    pub fn new_session(&mut self) {
        self.reset();
        self.frame = FrameCtx {
            seq: 0,
            epoch: self.frame.epoch.wrapping_add(1),
        };
    }

    /// Counter and epoch of the frame being assembled (or of the next one), as its prefix carries them.
    pub fn frame_ctx(&self) -> FrameCtx {
        self.frame
    }

    /// Go on with another framer's sequence: the next frame carries `frame` (from that framer's [Self::frame_ctx]
    /// once it was flushed), so a receiver with a counting [prefix](crate::prefix) sees no gap. For a host taking
    /// over a medium mid-stream (CLT-44). Drops what is in the current frame, as [Self::reset].
    pub fn continue_at(&mut self, frame: FrameCtx) {
        self.reset();
        self.frame = frame;
    }

    /// Write the prefix if nothing is in the frame yet. Must come after a write saved its `at_gap` state, so that
    /// restoring it takes the prefix out again when nothing else fits.
    #[inline(always)]
    fn begin_frame(&mut self, at_frame_start: bool) -> Result<(), ()> {
        if P::LEN > 0 && at_frame_start {
            P::write(self.frame, &mut self.wr).map_err(|_| ())?;
        }
        Ok(())
    }

    /// What the checksum of a Full covers besides the message: the frame's prefix and the head written at
    /// `head` (byte range in the frame). Copied out, the writer holds the buffer.
    #[inline(always)]
    fn full_header(&self, head: core::ops::Range<usize>) -> Header {
        let buf = self.wr.buf();
        Header::new(C::LEN_BYTES_FULL > 0, &buf[..P::LEN], &buf[head])
    }

    /// The same for the End of a split message, whose head is written after its checksum: encoded aside here.
    #[inline(always)]
    fn end_header(&self, user_kind: H::UserKind, remaining: usize) -> Header {
        if C::LEN_BYTES_SPLIT == 0 {
            return Header::new(false, &[], &[]);
        }
        let mut head = [0u8; MAX_COVERED_HEAD];
        let mut wr = BufWriter::new(&mut head);
        // fits: the same head was just written as Continue into the frame
        _ = H::write(MessageKind::End, user_kind, remaining, &mut wr);
        wr.align_byte();
        let len = wr.pos().0;
        Header::new(true, &self.wr.buf()[..P::LEN], &head[..len])
    }

    /// Whether the current frame has no bytes written into it yet.
    pub fn is_empty(&self) -> bool {
        self.wr.pos().0 == 0
    }

    /// Call repeatedly with the same message until Ok(true) is returned.
    /// While getting Ok(false), call [Self::flush] and send out the frame, before calling write again.
    /// Err(()) means the message is too large.
    ///
    /// Start and Continue extend till the end of a frame. End is written only if the remaining
    /// bytes fit into the frame together with checksum and tail, otherwise Continue is used and
    /// the rest goes into the next frame.
    /// Empty messages can be sent as well if an implementation requires it.
    pub fn write(&mut self, user_kind: H::UserKind, message: &[u8]) -> Result<bool, ()> {
        let at_gap = self.wr.save_state();
        // if nothing fits even into an empty frame, the message can never be sent
        let at_frame_start = self.wr.pos().0 == 0;
        if self.begin_frame(at_frame_start).is_err() {
            self.wr.restore_state(at_gap);
            self.state = State::Gap;
            return Err(());
        }
        // where the head goes, after the prefix
        let at_head = self.wr.save_state();
        let head_start = self.wr.pos().0;
        match self.state {
            State::Gap => {
                match H::write(MessageKind::Full, user_kind, message.len(), &mut self.wr) {
                    Ok(_) => {}
                    Err(WrError::OutOfBounds) => {
                        self.wr.restore_state(at_gap);
                        return if at_frame_start {
                            // too small assembly buffer that head doesn't fit even at the beginning
                            Err(())
                        } else {
                            Ok(false)
                        };
                    }
                    Err(WrError::TooBig) => {
                        self.wr.restore_state(at_gap);
                        self.state = State::Gap;
                        return Err(());
                    }
                }
                self.wr.align_byte();
                let buf_left = self.wr.bytes_left();
                if buf_left >= message.len() + C::LEN_BYTES_FULL + T::LEN_BYTES {
                    // message fits fully
                    let header = self.full_header(head_start..self.wr.pos().0);
                    _ = self.wr.write_raw_slice(message);
                    _ = C::write(header.get(), message, false, &mut self.wr);
                    _ = T::write(&mut self.wr);
                    return Ok(true);
                }
                // Start extends till the end of the frame and must leave at least 1 byte for End
                let take = buf_left.min(message.len().saturating_sub(1));
                if take == 0 {
                    self.wr.restore_state(at_gap);
                    if at_frame_start {
                        // nothing fits even into an empty frame, message can never be sent
                        return Err(());
                    }
                    // at least head + 1 byte must fit, otherwise use next frame
                    return Ok(false);
                }
                _ = self.wr.write_raw_slice(&message[..take]);
                let after_msg = self.wr.save_state();
                // message fits partially, change kind to Start
                self.wr.restore_state(at_head);
                _ = H::write(MessageKind::Start, user_kind, message.len(), &mut self.wr);
                self.wr.restore_state(after_msg);
                self.state = State::WroteN(take);
                Ok(false)
            }
            State::WroteN(n) => {
                let message_left = message.len() - n; // TODO: guard against user giving different message here or not?
                // Continue and End carry the remaining length, so that receiver can skip an End
                // whose Start was lost and continue with the rest of the frame
                if H::write(MessageKind::Continue, user_kind, message_left, &mut self.wr).is_err() {
                    self.wr.restore_state(at_gap);
                    if at_frame_start {
                        self.state = State::Gap;
                        return Err(());
                    }
                    return Ok(false);
                }
                self.wr.align_byte();
                let buf_left = self.wr.bytes_left();
                // End must fit into one frame together with checksum and tail
                if buf_left >= message_left + C::LEN_BYTES_SPLIT + T::LEN_BYTES {
                    let header = self.end_header(user_kind, message_left);
                    _ = self.wr.write_raw_slice(&message[n..]);
                    _ = C::write(header.get(), message, true, &mut self.wr);
                    _ = T::write(&mut self.wr);
                    let after_msg = self.wr.save_state();
                    // last bytes of message fit, change kind to End
                    // receiver already knows total length of a message, so it can correctly read only the remaining bytes
                    self.wr.restore_state(at_head);
                    _ = H::write(MessageKind::End, user_kind, message_left, &mut self.wr);
                    self.wr.restore_state(after_msg);
                    self.state = State::Gap;
                    return Ok(true);
                }
                // Continue extends till the end of the frame and must leave at least 1 byte for End
                let take = buf_left.min(message_left - 1);
                if take == 0 {
                    self.wr.restore_state(at_gap);
                    if at_frame_start {
                        // head alone fills the whole frame or End can never fit, message can never progress
                        self.state = State::Gap;
                        return Err(());
                    }
                    // at least head + 1 byte must fit, otherwise use next frame
                    return Ok(false);
                }
                _ = self.wr.write_raw_slice(&message[n..n + take]);
                self.state = State::WroteN(n + take);
                Ok(false)
            }
        }
    }

    /// Write a message only if it fits fully into the current frame, never splitting it.
    ///
    /// For stream media (UART, RTT, TCP), where frames are just chunks of bytes and their boundaries
    /// are not preserved, so that Start / Continue / End messages cannot be used.
    /// Ok(false) means the message does not fit into what is left of the current frame: call
    /// [Self::flush], send out the frame and call again. Err(()) means the message does not fit
    /// even into an empty frame and can never be sent.
    ///
    /// Must not be mixed with a partially written [Self::write].
    pub fn write_full(&mut self, user_kind: H::UserKind, message: &[u8]) -> Result<bool, ()> {
        debug_assert!(matches!(self.state, State::Gap));
        let at_gap = self.wr.save_state();
        let at_frame_start = self.wr.pos().0 == 0;
        let no_fit = |wr: &mut BufWriter<'i>| {
            wr.restore_state(at_gap);
            if at_frame_start { Err(()) } else { Ok(false) }
        };
        if self.begin_frame(at_frame_start).is_err() {
            return no_fit(&mut self.wr);
        }
        let head_start = self.wr.pos().0;
        match H::write(MessageKind::Full, user_kind, message.len(), &mut self.wr) {
            Ok(_) => {}
            Err(WrError::OutOfBounds) => return no_fit(&mut self.wr),
            Err(WrError::TooBig) => {
                self.wr.restore_state(at_gap);
                return Err(());
            }
        }
        self.wr.align_byte();
        if self.wr.bytes_left() < message.len() + C::LEN_BYTES_FULL + T::LEN_BYTES {
            return no_fit(&mut self.wr);
        }
        let header = self.full_header(head_start..self.wr.pos().0);
        _ = self.wr.write_raw_slice(message);
        _ = C::write(header.get(), message, false, &mut self.wr);
        _ = T::write(&mut self.wr);
        Ok(true)
    }

    /// Get next assembled frame size or 0 if called again without writing new messages.
    /// Frame bytes can be obtained via `&Self::buf()[..len]`.
    ///
    /// Note that frame can be shorter than the maximum, as at least head + length + 1 byte must fit.
    /// Or if called before whole frame is accumulated to lower delays.
    ///
    /// For frame media, this length must be preserved, otherwise receiver will not work.
    /// This is usually trivially achieved on e.g., USB or CAN.
    ///
    /// For stream media, frame is simply next chunk of bytes to send. On real hardware having a
    /// chunk is more efficient than an actual stream of bytes, as it can be fed into DMA.
    pub fn flush(&mut self) -> usize {
        let len = self.wr.pos().0;
        self.wr.reset();
        if len > 0 {
            self.frame.seq = self.frame.seq.wrapping_add(1);
        }
        len
    }

    /// Get a reference to the internal buffer.
    /// Call `flush()` first to get the length of the frame.
    pub fn buf(&self) -> &[u8] {
        self.wr.buf()
    }
}

use core::marker::PhantomData;

use shrink_wrap::{BufWriter, buf_writer::BufWriterState};

use crate::traits::{Checksum, Head, MessageKind, Tail, WrError};

pub struct Tx<'i, H, C, T> {
    wr: BufWriter<'i>,
    state: State,
    _phantom_h: PhantomData<H>,
    _phantom_c: PhantomData<C>,
    _phantom_t: PhantomData<T>,
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
}

impl TxState {
    /// Same as [Tx::is_empty], without re-creating [Tx].
    pub fn is_empty(&self) -> bool {
        self.wr.pos().0 == 0
    }
}

impl<'b, 'i: 'b, H: Head, C: Checksum, T: Tail> Tx<'i, H, C, T>
where
    H::UserKind: Copy,
{
    /// Create new framer from the provided assembly buffer.
    /// Buffer must be exactly the length of the maximum frame (or DMA size).
    pub fn new(assembly_buf: &'i mut [u8]) -> Self {
        #[cfg(not(test))] // to catch potentially incorrect H::MIN_FRAME_SIZE in tests
        debug_assert!(assembly_buf.len() >= H::MIN_FRAME_SIZE);
        Tx {
            wr: BufWriter::new(assembly_buf),
            state: State::Gap,
            _phantom_h: PhantomData,
            _phantom_c: PhantomData,
            _phantom_t: PhantomData,
        }
    }

    /// Re-create framer from the same assembly buffer and state previously obtained from [Self::into_parts].
    pub fn from_parts(assembly_buf: &'i mut [u8], parts: TxState) -> Self {
        let mut wr = BufWriter::new(assembly_buf);
        wr.restore_state(parts.wr);
        Tx {
            wr,
            state: parts.state,
            _phantom_h: PhantomData,
            _phantom_c: PhantomData,
            _phantom_t: PhantomData,
        }
    }

    /// Release the assembly buffer, keeping all the state needed to continue later with [Self::from_parts].
    pub fn into_parts(self) -> TxState {
        TxState {
            wr: self.wr.save_state(),
            state: self.state,
        }
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
                    _ = self.wr.write_raw_slice(message);
                    _ = C::write(message, false, &mut self.wr);
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
                self.wr.restore_state(at_gap);
                _ = H::write(MessageKind::Start, user_kind, message.len(), &mut self.wr);
                self.wr.restore_state(after_msg);
                self.state = State::WroteN(take);
                Ok(false)
            }
            State::WroteN(n) => {
                let message_left = message.len() - n; // TODO: guard agains user giving different message here or not?
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
                    _ = self.wr.write_raw_slice(&message[n..]);
                    _ = C::write(message, true, &mut self.wr);
                    _ = T::write(&mut self.wr);
                    let after_msg = self.wr.save_state();
                    // last bytes of message fit, change kind to End
                    // receiver already knows total length of a message, so it can correctly read only the remaining bytes
                    self.wr.restore_state(at_gap);
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

    /// Get next assembled frame size or 0 if called again without writing new messages.
    /// Frame bytes can be obtained via `&Self::buf()[..len]`.
    ///
    /// Note that frame can be shorter than the maximum, as at least head + length + 1 byte must fit.
    /// Or if called before whole frame is accumulated to lower delays.
    ///
    /// For frame media, this length must be preserved, otherise receiver will not work.
    /// This is usually trivially achieved on e.g., USB or CAN.
    ///
    /// For stream media, frame is simply next chunk of bytes to send. On real hardware having a
    /// chunk is more efficient than an actual stream of bytes, as it can be fed into DMA.
    pub fn flush(&mut self) -> usize {
        let len = self.wr.pos().0;
        self.wr.reset();
        len
    }

    /// Get a reference to the internal buffer.
    /// Call `flush()` first to get the length of the frame.
    pub fn buf(&self) -> &[u8] {
        self.wr.buf()
    }
}

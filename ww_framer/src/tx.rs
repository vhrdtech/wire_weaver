use core::marker::PhantomData;

use shrink_wrap::BufWriter;

use crate::traits::{Checksum, Head, MessageKind, Tail, WrError};

pub struct Tx<'i, H, C, T> {
    wr: BufWriter<'i>,
    state: State,
    _phantom_h: PhantomData<H>,
    _phantom_c: PhantomData<C>,
    _phantom_t: PhantomData<T>,
}

enum State {
    Gap,
    WroteN(usize),
}

impl<'b, 'i: 'b, H: Head, C: Checksum, T: Tail> Tx<'i, H, C, T>
where
    H::UserKind: Copy,
{
    /// Create new framer from the provided assembly buffer.
    /// Buffer must be exactly the length of the maximum frame (or DMA size).
    /// TODO: min buffer size
    pub fn new(assembly_buf: &'i mut [u8]) -> Self {
        debug_assert!(assembly_buf.len() >= 4);
        Tx {
            wr: BufWriter::new(assembly_buf),
            state: State::Gap,
            _phantom_h: PhantomData,
            _phantom_c: PhantomData,
            _phantom_t: PhantomData,
        }
    }

    /// Call repeatedly with the same message until Ok(true) is returned.
    /// While getting Ok(false), call [Self::flush] and send out the frame, before calling write again.
    /// Err(()) means the message is too large.
    /// Empty messages can be sent as well if an implementation requires it.
    pub fn write(&mut self, user_kind: H::UserKind, message: &[u8]) -> Result<bool, ()> {
        let at_gap = self.wr.save_state();
        match self.state {
            State::Gap => {
                match H::write(MessageKind::Full, user_kind, message.len(), &mut self.wr) {
                    Ok(_) => {}
                    Err(WrError::OutOfBounds) => {
                        if self.wr.pos().0 == 0 {
                            // too small assembly buffer that head doesn't fit even at the beginning
                            return Err(());
                        }
                        self.wr.restore_state(at_gap);
                        return Ok(false);
                    }
                    Err(WrError::TooBig) => {
                        self.wr.restore_state(at_gap);
                        self.state = State::Gap;
                        return Err(());
                    }
                }
                self.wr.align_byte();
                let buf_left = self.wr.bytes_left();
                if buf_left == 0 {
                    if self.wr.pos().0 == 0 {
                        return Err(());
                    }
                    // at least head + length + 1 byte must fit, otherwise use next frame
                    self.wr.restore_state(at_gap);
                    Ok(false)
                } else if buf_left < message.len() + C::LEN_BYTES_FULL + T::LEN_BYTES {
                    _ = self.wr.write_raw_slice(&message[..buf_left]);
                    let after_msg = self.wr.save_state();
                    // message fits partially, change kind to Start
                    self.wr.restore_state(at_gap);
                    _ = H::write(MessageKind::Start, user_kind, message.len(), &mut self.wr);
                    self.wr.restore_state(after_msg);
                    self.state = State::WroteN(buf_left);
                    Ok(false)
                } else {
                    // message fits fully
                    _ = self.wr.write_raw_slice(message);
                    _ = C::write(&message, false, &mut self.wr);
                    _ = T::write(&mut self.wr);
                    Ok(true)
                }
            }
            State::WroteN(n) => {
                // if Continue message spans till end of frame anyway, no need to serialize real length here
                if H::write(MessageKind::Continue, user_kind, 0, &mut self.wr).is_err() {
                    self.wr.restore_state(at_gap);
                    return Ok(false);
                }
                self.wr.align_byte();
                let message_left = message.len() - n; // TODO: guard agains user giving different message here or not?
                let buf_left = self.wr.bytes_left();
                if buf_left == 0 {
                    // at least head + length + 1 byte must fit, otherwise use next frame
                    self.wr.restore_state(at_gap);
                    Ok(false)
                } else if buf_left < message_left + C::LEN_BYTES_SPLIT + T::LEN_BYTES {
                    _ = self.wr.write_raw_slice(&message[n..n + buf_left]);
                    self.state = State::WroteN(n + buf_left);
                    Ok(false)
                } else {
                    _ = self.wr.write_raw_slice(&message[n..]);
                    _ = C::write(message, true, &mut self.wr);
                    _ = T::write(&mut self.wr);
                    let after_msg = self.wr.save_state();
                    // last bytes of message fit, change kind to End
                    // receiver already knows total length of a message, so it can correctly read only the remaining bytes
                    self.wr.restore_state(at_gap);
                    _ = H::write(MessageKind::End, user_kind, 0, &mut self.wr);
                    self.wr.restore_state(after_msg);
                    self.state = State::Gap;
                    Ok(true)
                }
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

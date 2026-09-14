//! Glue between the sans-IO [Core](super::core::Core) (which speaks in link messages) and a
//! [ww_framer] configuration chosen by the transport wrapper (which speaks in frames).

use ww_framer::traits::{Checksum, Head, Tail};
use ww_framer::{FramedRxOwned, TxOwned};

/// Owns a framer pair and turns [Output::Send](super::core::Output::Send) / [Output::Flush](super::core::Output::Flush)
/// into frames, and received frames into `(kind, payload)` messages for [Input::Message](super::core::Input::Message).
pub(crate) struct Framing<H: Head, C: Checksum, T: Tail> {
    tx: TxOwned<H, C, T>,
    rx: FramedRxOwned<H, C, T>,
}

impl<H: Head<UserKind = u8>, C: Checksum, T: Tail> Framing<H, C, T> {
    /// `frame_size` is what the medium carries as one unit (USB packet, datagram, ...),
    /// `max_message_size` is the largest message expected to be received.
    pub fn new(frame_size: usize, max_message_size: usize) -> Self {
        Framing {
            tx: TxOwned::new(frame_size),
            rx: FramedRxOwned::new(max_message_size + frame_size),
        }
    }

    /// Write a message, emitting complete frames into `frames` as they fill up.
    /// Partial frame is kept until [Self::flush].
    pub fn write(
        &mut self,
        kind: u8,
        payload: &[u8],
        frames: &mut Vec<Vec<u8>>,
    ) -> Result<(), FramingError> {
        loop {
            match self.tx.write(kind, payload) {
                Ok(true) => return Ok(()),
                Ok(false) => match self.tx.flush_to_vec() {
                    Some(frame) => frames.push(frame),
                    None => return Err(FramingError::CannotProgress),
                },
                Err(()) => return Err(FramingError::MessageTooBig(payload.len())),
            }
        }
    }

    /// Emit the current partial frame, if any.
    pub fn flush(&mut self, frames: &mut Vec<Vec<u8>>) {
        if let Some(frame) = self.tx.flush_to_vec() {
            frames.push(frame);
        }
    }

    /// Stage a received frame; then call [Self::next_message] until it returns None.
    pub fn stage(&mut self, frame: &[u8]) -> Result<(), FramingError> {
        self.rx.stage(frame).map_err(|_| FramingError::RxOverflow)
    }

    pub fn next_message(&mut self) -> Option<(u8, &[u8])> {
        self.rx.reassemble();
        self.rx.message()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum FramingError {
    #[error("message of {0} bytes is too big for the framer")]
    MessageTooBig(usize),
    #[error("framer cannot make progress: nothing fits into an empty frame")]
    CannotProgress,
    #[error("rx assembly buffer overflow")]
    RxOverflow,
}

#[cfg(test)]
mod tests {
    use super::*;

    type F = Framing<ww_link::Head, ww_link::Checksum, ww_link::Tail>;

    #[test]
    fn small_messages_share_a_frame_and_large_ones_split() {
        let mut host = F::new(16, 64);
        let mut dev = F::new(16, 64);
        let mut frames = vec![];

        host.write(0, &[1, 2, 3], &mut frames).unwrap();
        host.write(1, &[4, 5], &mut frames).unwrap();
        assert!(frames.is_empty(), "partial frame is held until flush");
        host.flush(&mut frames);
        assert_eq!(frames.len(), 1);

        let big: Vec<u8> = (0..40).collect();
        host.write(ww_link::Kind::Ping as u8, &big, &mut frames)
            .unwrap();
        host.flush(&mut frames);
        assert!(frames.len() > 2, "large message spans several frames");

        let mut got = vec![];
        for f in frames.drain(..) {
            dev.stage(&f).unwrap();
            while let Some((k, p)) = dev.next_message() {
                got.push((k, p.to_vec()));
            }
        }
        assert_eq!(
            got,
            [
                (0, vec![1, 2, 3]),
                (1, vec![4, 5]),
                (ww_link::Kind::Ping as u8, big)
            ]
        );
    }

    #[test]
    fn oversized_message_is_dropped_by_receiver_and_next_one_delivered() {
        // Tx will split anything the head can describe; respecting the peer's max message length is the
        // link layer's job. If it is violated, the receiver drops that message and stays in sync.
        let mut host = F::new(16, 64);
        let mut dev = F::new(16, 64);
        let mut frames = vec![];
        host.write(0, &[0u8; 4000], &mut frames).unwrap();
        host.write(1, &[9], &mut frames).unwrap();
        host.flush(&mut frames);
        let mut got = vec![];
        for f in &frames {
            dev.stage(f).unwrap();
            while let Some((k, p)) = dev.next_message() {
                got.push((k, p.to_vec()));
            }
        }
        assert_eq!(got, [(1, vec![9])]);
    }
}

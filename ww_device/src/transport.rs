//! Async message-level transport, mirroring the host side (`wire_weaver_client`): the link core speaks
//! `(kind, payload)` pairs, how they get onto the medium is up to the implementation. Frame-based
//! media (USB, CAN, UDP, ...) use [FramedTx] and [FramedRx] over a packet level [PacketSink] and
//! [PacketSource].

use core::future::Future;

use ww_link::{UsbChecksum, UsbHead, UsbTail};

use crate::fmt::warn;

/// Framer used by [FramedTx], USB configuration from [ww_link].
pub type TxFramer<'a> = ww_framer::Tx<'a, UsbHead, UsbChecksum, UsbTail>;
/// Framer used by [FramedRx], USB configuration from [ww_link].
pub type RxFramer<'a> = ww_framer::FramedRx<'a, UsbHead, UsbChecksum, UsbTail>;

/// Message tx half.
pub trait MessageTx {
    type Error;

    /// Write a message. Implementations may hold it back until a frame is full or [flush](Self::flush) is called.
    fn write_message(
        &mut self,
        kind: u8,
        message: &[u8],
    ) -> impl Future<Output = Result<(), Self::Error>>;

    /// Send whatever is held back, even if the frame is not full. No-op if nothing is pending.
    fn flush(&mut self) -> impl Future<Output = Result<(), Self::Error>>;

    /// Drop anything held back, called when the medium goes down.
    fn reset(&mut self);
}

/// Message rx half.
///
/// Receiving is split into a cancel-safe wait and a separate access to the message, so that the
/// wait can be used in a `select` with other futures, and a message can be processed without
/// holding a borrow from the future.
pub trait MessageRx {
    type Error;

    /// Wait until [message](Self::message) returns Some.
    ///
    /// Must be cancel-safe: dropping the future must not lose data. This is natural when any
    /// partially received state lives in `self` (e.g., a framer's staging buffer), not in the future.
    ///
    /// An error means the medium went down, [wait_connected](Self::wait_connected) is awaited next.
    fn wait_message(&mut self) -> impl Future<Output = Result<(), Self::Error>>;

    /// Message received by [wait_message](Self::wait_message), if any.
    fn message(&self) -> Option<(u8, &[u8])>;

    /// Done with the current message, next [wait_message](Self::wait_message) moves to the next one.
    fn consume(&mut self);

    /// Wait until the medium is usable (e.g., USB configured by the host). Must be cancel-safe.
    fn wait_connected(&mut self) -> impl Future<Output = ()>;

    /// Drop anything partially received, called when the medium goes down.
    fn reset(&mut self);

    /// Longest message that can be received.
    fn max_message_len(&self) -> usize;
}

/// Packet (frame) level sink, e.g., USB IN endpoint.
pub trait PacketSink {
    type Error;

    /// Write one packet. Should fail eventually if the other side does not read, so that the device
    /// notices a host that is gone without disconnecting.
    fn write_packet(&mut self, packet: &[u8]) -> impl Future<Output = Result<(), Self::Error>>;
}

/// Packet (frame) level source, e.g., USB OUT endpoint.
pub trait PacketSource {
    type Error;

    /// Largest packet that can be received.
    fn max_packet_len(&self) -> usize;

    /// Read one packet into `buf`, which is at least [max_packet_len](Self::max_packet_len) long.
    /// Must be cancel-safe.
    fn read_packet(&mut self, buf: &mut [u8]) -> impl Future<Output = Result<usize, Self::Error>>;

    /// Wait until the medium is usable. Must be cancel-safe.
    fn wait_connected(&mut self) -> impl Future<Output = ()>;
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum FramedError<E> {
    /// Packet sink or source failed.
    Transport(E),
    /// Message does not fit into the framer at all, a bug or wrong buffer sizes.
    Framing,
}

/// [MessageTx] over a [PacketSink], packing messages into frames with [ww_framer].
pub struct FramedTx<'a, P> {
    framer: TxFramer<'a>,
    sink: P,
}

impl<'a, P: PacketSink> FramedTx<'a, P> {
    /// `frame_buf` must be exactly the maximum packet size.
    pub fn new(sink: P, frame_buf: &'a mut [u8]) -> Self {
        FramedTx {
            framer: TxFramer::new(frame_buf),
            sink,
        }
    }

    pub fn sink_mut(&mut self) -> &mut P {
        &mut self.sink
    }

    /// Returns false if there was nothing to send.
    async fn send_frame(&mut self) -> Result<bool, FramedError<P::Error>> {
        let len = self.framer.flush();
        if len == 0 {
            return Ok(false);
        }
        let frame = &self.framer.buf()[..len];
        self.sink
            .write_packet(frame)
            .await
            .map_err(FramedError::Transport)?;
        Ok(true)
    }
}

impl<P: PacketSink> MessageTx for FramedTx<'_, P> {
    type Error = FramedError<P::Error>;

    async fn write_message(&mut self, kind: u8, message: &[u8]) -> Result<(), Self::Error> {
        loop {
            match self.framer.write(kind, message) {
                Ok(true) => return Ok(()),
                Ok(false) => {
                    // frame is full (or the message continues into the next one): send and retry
                    if !self.send_frame().await? {
                        return Err(FramedError::Framing);
                    }
                }
                Err(()) => return Err(FramedError::Framing),
            }
        }
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        self.send_frame().await.map(|_| ())
    }

    fn reset(&mut self) {
        self.framer.reset();
    }
}

/// [MessageRx] over a [PacketSource], reassembling messages with [ww_framer].
/// Packets are received directly into the framer's buffer, without an extra copy.
pub struct FramedRx<'a, P> {
    framer: RxFramer<'a>,
    source: P,
    max_message_len: usize,
    /// A message is ready and not consumed yet
    ready: bool,
}

impl<'a, P: PacketSource> FramedRx<'a, P> {
    /// `assembly_buf` must hold one maximum message plus one maximum packet, the rest is reported
    /// to the host as the maximum message length. Use [RxBuffer::assembly_buf](crate::RxBuffer::assembly_buf)
    /// to get exactly the desired maximum message length.
    pub fn new(source: P, assembly_buf: &'a mut [u8]) -> Self {
        let max_packet_len = source.max_packet_len();
        debug_assert!(assembly_buf.len() > max_packet_len);
        let max_message_len = assembly_buf.len().saturating_sub(max_packet_len);
        FramedRx {
            framer: RxFramer::new(assembly_buf),
            source,
            max_message_len,
            ready: false,
        }
    }

    pub fn source_mut(&mut self) -> &mut P {
        &mut self.source
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum RxError<E> {
    Transport(E),
}

impl<P: PacketSource> MessageRx for FramedRx<'_, P> {
    type Error = RxError<P::Error>;

    async fn wait_message(&mut self) -> Result<(), Self::Error> {
        loop {
            if self.ready {
                return Ok(());
            }
            // Consumes the previous message (if any) and tries to assemble the next one from what
            // is already staged, before reading more.
            self.framer.reassemble();
            if self.framer.message().is_some() {
                self.ready = true;
                return Ok(());
            }
            let max_packet_len = self.source.max_packet_len();
            if self.framer.free() < max_packet_len {
                // only possible if the host sent a message larger than advertised
                warn!("rx assembly buffer overflow, dropping partial message");
                self.framer.reset();
            }
            let buf = &mut self.framer.staging_buf()[..max_packet_len];
            let len = self
                .source
                .read_packet(buf)
                .await
                .map_err(RxError::Transport)?;
            // cannot fail, buf was taken from the free area
            _ = self.framer.commit_staged(len);
        }
    }

    fn message(&self) -> Option<(u8, &[u8])> {
        if self.ready {
            self.framer.message()
        } else {
            None
        }
    }

    fn consume(&mut self) {
        self.ready = false;
    }

    async fn wait_connected(&mut self) {
        self.source.wait_connected().await
    }

    fn reset(&mut self) {
        self.framer.reset();
        self.ready = false;
    }

    fn max_message_len(&self) -> usize {
        self.max_message_len
    }
}

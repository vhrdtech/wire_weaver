//! Glue for devices without an async runtime (bare-metal super-loop, RTIC, interrupt driven USB stacks).
//!
//! Received packets are pushed in with [Server::on_packet] (e.g., from a USB interrupt or a polling
//! loop), time is advanced with [Server::poll], and frames go out through a blocking [PacketSink].
//! Uses the same [DeviceLink] core as the async [Server](crate::Server), only the IO differs.
//!
//! ```ignore
//! loop {
//!     let now = Instant::from_micros(timer.now_us());
//!     if let Some(packet) = usb.poll_out_packet() {
//!         server.on_packet(now, packet, &mut state);
//!     }
//!     server.poll(now);
//!     if let Some(chunk) = uart.try_read() {
//!         let (mut sink, scratch) = server.sink(now);
//!         state.send_uart_chunk(chunk, scratch, &mut sink);
//!     }
//!     // sleep until the next interrupt or server.poll_timeout()
//! }
//! ```

use wire_weaver::{MessageSink, WireWeaverApiBackend};

use crate::link::{DeviceLink, LinkConfig, LinkEvent, Received, SendError, Transmit};
use crate::time::Instant;
use crate::transport::{RxFramer, TxFramer};

/// Blocking packet (frame) level sink, e.g., USB IN endpoint.
pub trait PacketSink {
    type Error;

    /// Write one packet, blocking until it is accepted. Should fail eventually if the other side
    /// does not read, so that the device notices a host that is gone without disconnecting.
    fn write_packet(&mut self, packet: &[u8]) -> Result<(), Self::Error>;
}

pub struct Server<'a, P> {
    link: DeviceLink<'a>,
    tx: TxFramer<'a>,
    sink: P,
    rx: RxFramer<'a>,
    scratch: &'a mut [u8],
}

impl<'a, P: PacketSink> Server<'a, P> {
    /// * `tx_frame_buf` must be exactly the maximum packet size.
    /// * `rx_assembly_buf` must hold one maximum message plus one maximum packet, the rest is
    ///   reported to the host as the maximum message length. Use
    ///   [RxBuffer::assembly_buf](crate::RxBuffer::assembly_buf)`(tx_frame_buf.len())` to get exactly
    ///   the desired maximum message length.
    /// * `scratch` is used to serialize replies.
    pub fn new(
        config: LinkConfig<'a>,
        sink: P,
        tx_frame_buf: &'a mut [u8],
        rx_assembly_buf: &'a mut [u8],
        scratch: &'a mut [u8],
    ) -> Self {
        let max_message_len = rx_assembly_buf.len().saturating_sub(tx_frame_buf.len());
        Server {
            link: DeviceLink::new(config, max_message_len),
            tx: TxFramer::new(tx_frame_buf),
            sink,
            rx: RxFramer::new(rx_assembly_buf),
            scratch,
        }
    }

    pub fn link(&self) -> &DeviceLink<'a> {
        &self.link
    }

    pub fn is_up(&self) -> bool {
        self.link.is_up()
    }

    pub fn sink_mut(&mut self) -> &mut P {
        &mut self.sink
    }

    /// Medium became usable (e.g., USB configured by the host).
    pub fn on_transport_up(&mut self, now: Instant) {
        self.rx.reset();
        self.tx.reset();
        self.link.on_transport_up(now);
    }

    /// Medium went down.
    pub fn on_transport_down(&mut self) -> Option<LinkEvent> {
        self.link.on_transport_down();
        self.link.poll_event()
    }

    /// Feed a received packet (frame), all messages it completes are processed right away.
    /// Returns a link state change, if any.
    pub fn on_packet<B: WireWeaverApiBackend>(
        &mut self,
        now: Instant,
        packet: &[u8],
        backend: &mut B,
    ) -> Option<LinkEvent> {
        if self.rx.stage(packet).is_err() {
            // only possible if the host sent a message larger than advertised
            warn!("rx assembly buffer overflow, dropping partial message");
            self.rx.reset();
            _ = self.rx.stage(packet);
        }
        let mut event = None;
        loop {
            self.rx.reassemble();
            let Some((kind, message)) = self.rx.message() else {
                break;
            };
            let mut ctx = Ctx {
                link: &mut self.link,
                tx: &mut self.tx,
                sink: &mut self.sink,
                now,
            };
            ctx.on_message(kind, message, backend, self.scratch);
            ctx.drain_transmit(self.scratch);
            event = self.link.poll_event().or(event);
        }
        event
    }

    /// Feed a whole message, for media that do not need [ww_framer] (the framer is not used then).
    pub fn on_message<B: WireWeaverApiBackend>(
        &mut self,
        now: Instant,
        kind: u8,
        message: &[u8],
        backend: &mut B,
    ) -> Option<LinkEvent> {
        let mut ctx = Ctx {
            link: &mut self.link,
            tx: &mut self.tx,
            sink: &mut self.sink,
            now,
        };
        ctx.on_message(kind, message, backend, self.scratch);
        ctx.drain_transmit(self.scratch);
        self.link.poll_event()
    }

    /// Advance timers: flush accumulated data, send pings, detect a silent host.
    /// Call at least at [Self::poll_timeout], calling more often is harmless.
    pub fn poll(&mut self, now: Instant) -> Option<LinkEvent> {
        self.link.handle_timeout(now);
        let (mut ctx, scratch) = self.parts(now);
        ctx.drain_transmit(scratch);
        self.link.poll_event()
    }

    /// When [Self::poll] needs to be called next, None if only incoming packets matter.
    pub fn poll_timeout(&self) -> Option<Instant> {
        self.link.poll_timeout()
    }

    /// Sink to send stream updates or deferred replies, and scratch space to serialize them.
    pub fn sink(&mut self, now: Instant) -> (Sink<'_, 'a, P>, &mut [u8]) {
        (
            Sink {
                ctx: Ctx {
                    link: &mut self.link,
                    tx: &mut self.tx,
                    sink: &mut self.sink,
                    now,
                },
            },
            self.scratch,
        )
    }

    /// Send a data message (serialized `ww_client_server::Event`).
    pub fn send(&mut self, now: Instant, message: &[u8]) -> Result<(), SendError> {
        self.parts(now).0.send_message(message)
    }

    /// Tell the host that the device is going away and send it out right away.
    pub fn disconnect(&mut self, now: Instant, reason: ww_link::DisconnectReason) {
        self.link.disconnect(reason);
        let (mut ctx, scratch) = self.parts(now);
        ctx.drain_transmit(scratch);
    }

    fn parts(&mut self, now: Instant) -> (Ctx<'_, 'a, P>, &mut [u8]) {
        (
            Ctx {
                link: &mut self.link,
                tx: &mut self.tx,
                sink: &mut self.sink,
                now,
            },
            self.scratch,
        )
    }
}

/// Everything but the rx framer and scratch, so that a received message and scratch can be borrowed
/// at the same time.
struct Ctx<'s, 'a, P> {
    link: &'s mut DeviceLink<'a>,
    tx: &'s mut TxFramer<'a>,
    sink: &'s mut P,
    now: Instant,
}

impl<'a, P: PacketSink> Ctx<'_, 'a, P> {
    fn on_message<B: WireWeaverApiBackend>(
        &mut self,
        kind: u8,
        message: &[u8],
        backend: &mut B,
        scratch: &mut [u8],
    ) {
        match self.link.handle_message(self.now, kind, message) {
            Some(Received::Data(request)) => {
                let mut sink = Sink {
                    ctx: self.reborrow(),
                };
                let reply = match backend.process_bytes(&mut sink, request, scratch) {
                    Ok(reply) => reply,
                    Err(e) => {
                        error!("process_bytes failed: {:?}", e);
                        let Some(reply) = crate::generic_error_reply(request, scratch) else {
                            return;
                        };
                        reply
                    }
                };
                if !reply.is_empty() {
                    _ = self.send_message(reply);
                }
            }
            Some(Received::Loopback { repeat, seq, data }) => {
                for seq in crate::loopback_seqs(repeat, seq) {
                    let Some((kind, bytes)) = crate::loopback_reply(seq, data, scratch) else {
                        return;
                    };
                    if self.write_message(kind, bytes).is_err() {
                        return;
                    }
                    self.link.on_data_written(self.now);
                }
            }
            None => {}
        }
    }

    fn reborrow(&mut self) -> Ctx<'_, 'a, P> {
        Ctx {
            link: self.link,
            tx: self.tx,
            sink: self.sink,
            now: self.now,
        }
    }

    fn send_message(&mut self, message: &[u8]) -> Result<(), SendError> {
        self.link.check_send(message.len())?;
        self.write_message(DeviceLink::data_kind(), message)?;
        self.link.on_data_written(self.now);
        Ok(())
    }

    fn write_message(&mut self, kind: u8, message: &[u8]) -> Result<(), SendError> {
        loop {
            match self.tx.write(kind, message) {
                Ok(true) => return Ok(()),
                Ok(false) => {
                    if !self.send_frame()? {
                        error!("framing error: cannot progress");
                        return Err(SendError::TooBig);
                    }
                }
                Err(()) => {
                    error!("framing error: message too big");
                    return Err(SendError::TooBig);
                }
            }
        }
    }

    /// Returns false if there was nothing to send.
    fn send_frame(&mut self) -> Result<bool, SendError> {
        let len = self.tx.flush();
        if len == 0 {
            return Ok(false);
        }
        if self.sink.write_packet(&self.tx.buf()[..len]).is_err() {
            warn!("write failed, transport down");
            self.link.on_transport_down();
            return Err(SendError::Transport);
        }
        Ok(true)
    }

    fn drain_transmit(&mut self, scratch: &mut [u8]) {
        while let Some(t) = self.link.poll_transmit(self.now, scratch) {
            let r = match t {
                Transmit::Message { kind, bytes } => self.write_message(kind, bytes),
                Transmit::Flush => self.send_frame().map(|_| ()),
            };
            if r.is_err() {
                break;
            }
        }
    }
}

/// Writes data messages into the framer, obtained with [Server::sink] or passed to the backend.
pub struct Sink<'s, 'a, P> {
    ctx: Ctx<'s, 'a, P>,
}

impl<P: PacketSink> Sink<'_, '_, P> {
    pub fn is_up(&self) -> bool {
        self.ctx.link.is_up()
    }

    /// Send a data message (serialized `ww_client_server::Event`). It goes out when a frame is full or
    /// on the next [Server::poll] after the accumulation window.
    pub fn send_message(&mut self, message: &[u8]) -> Result<(), SendError> {
        self.ctx.send_message(message)
    }
}

/// Completes on the first poll, as writes are blocking.
impl<P: PacketSink> MessageSink for Sink<'_, '_, P> {
    async fn send(&mut self, message: &[u8]) -> Result<(), ()> {
        self.send_message(message).map_err(|e| {
            warn!("MessageSink::send failed: {:?}", e);
        })
    }
}

//! Async glue: [DeviceLink] + [MessageTx] / [MessageRx] + a user backend. The event loop itself stays in
//! user code, see [Server] for the intended use.

use core::future::{Future, pending};

use embassy_futures::select::{Either, select};
use wire_weaver::{MessageSink, WireWeaverAsyncApiBackend};

use crate::link::{DeviceLink, LinkConfig, LinkEvent, Phase, Received, SendError, Transmit};
use crate::time::Instant;
use crate::transport::{MessageRx, MessageTx};

/// Time source and timer for [Server].
pub trait Clock {
    fn now(&self) -> Instant;
    fn wait_until(&self, at: Instant) -> impl Future<Output = ()>;
}

/// [Clock] using embassy-time.
#[cfg(feature = "embassy-time")]
#[derive(Copy, Clone, Default)]
pub struct EmbassyClock;

#[cfg(feature = "embassy-time")]
impl Clock for EmbassyClock {
    fn now(&self) -> Instant {
        Instant::from_micros(embassy_time::Instant::now().as_micros())
    }

    async fn wait_until(&self, at: Instant) {
        embassy_time::Timer::at(embassy_time::Instant::from_micros(at.as_micros())).await
    }
}

/// What [Server::wait] observed, to be passed to [Server::handle].
#[must_use = "pass to Server::handle()"]
#[derive(Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct Ready(ReadyKind);

#[derive(Copy, Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
enum ReadyKind {
    TransportUp,
    TransportDown,
    Message,
    Timeout,
}

/// Device side event loop building block.
///
/// The loop lives in user code, so that any other async source can be selected on alongside:
/// ```ignore
/// loop {
///     match select3(server.wait(), uart_rx.wait(), ticker.next()).await {
///         Either3::First(ready) => {
///             if let Some(event) = server.handle(ready, &mut state).await {
///                 // LinkEvent::Up / Down, e.g., to drive an LED
///             }
///         }
///         Either3::Second(chunk) => {
///             let (mut sink, scratch) = server.sink();
///             state.send_uart_chunk(chunk, scratch, &mut sink).await;
///         }
///         Either3::Third(_) => { /* periodic stream updates through server.sink() */ }
///     }
/// }
/// ```
/// [wait](Self::wait) is cancel-safe and only waits; [handle](Self::handle) does all the work and
/// should run to completion (it writes to the medium). If nothing else needs to be selected on,
/// [run](Self::run) is the whole loop.
///
/// Writes are awaited inline, so while a backend handler or a sink write awaits, nothing is received.
/// This is fine, as the host always reads independently of writing, but backend handlers should not
/// block for long — use deferred replies instead.
pub struct Server<'a, Tx, Rx, C> {
    link: DeviceLink<'a>,
    tx: Tx,
    rx: Rx,
    clock: C,
    /// Backend replies and control messages are serialized here
    scratch: &'a mut [u8],
}

impl<'a, Tx: MessageTx, Rx: MessageRx, C: Clock> Server<'a, Tx, Rx, C> {
    /// `scratch` is used to serialize replies, so it limits their size, together with the host's
    /// maximum message length.
    pub fn new(config: LinkConfig<'a>, tx: Tx, rx: Rx, clock: C, scratch: &'a mut [u8]) -> Self {
        let max_message_len = rx.max_message_len();
        Server {
            link: DeviceLink::new(config, max_message_len),
            tx,
            rx,
            clock,
            scratch,
        }
    }

    pub fn link(&self) -> &DeviceLink<'a> {
        &self.link
    }

    pub fn is_up(&self) -> bool {
        self.link.is_up()
    }

    pub fn tx_mut(&mut self) -> &mut Tx {
        &mut self.tx
    }

    pub fn rx_mut(&mut self) -> &mut Rx {
        &mut self.rx
    }

    /// Wait for something to [handle](Self::handle): a received message, a timer or the medium
    /// going up or down. Cancel-safe.
    pub async fn wait(&mut self) -> Ready {
        if self.link.wants_transmit() {
            // e.g., after Self::disconnect() was called outside of handle()
            return Ready(ReadyKind::Timeout);
        }
        if self.link.phase() == Phase::Down {
            self.rx.wait_connected().await;
            return Ready(ReadyKind::TransportUp);
        }
        let deadline = self.link.poll_timeout();
        let clock = &self.clock;
        let timer = async {
            match deadline {
                Some(at) => clock.wait_until(at).await,
                None => pending().await,
            }
        };
        match select(self.rx.wait_message(), timer).await {
            Either::First(Ok(())) => Ready(ReadyKind::Message),
            Either::First(Err(_)) => Ready(ReadyKind::TransportDown),
            Either::Second(()) => Ready(ReadyKind::Timeout),
        }
    }

    /// Act on what [wait](Self::wait) returned: link setup, requests to the backend, pings, flushes.
    /// Returns a link state change, if any.
    pub async fn handle<B: WireWeaverAsyncApiBackend>(
        &mut self,
        ready: Ready,
        backend: &mut B,
    ) -> Option<LinkEvent> {
        let now = self.clock.now();
        match ready.0 {
            ReadyKind::TransportUp => {
                self.rx.reset();
                self.tx.reset();
                self.link.on_transport_up(now);
            }
            ReadyKind::TransportDown => {
                warn!("receive failed, transport down");
                self.link.on_transport_down();
            }
            ReadyKind::Timeout => self.link.handle_timeout(now),
            ReadyKind::Message => {
                if let Some((kind, message)) = self.rx.message() {
                    match self.link.handle_message(now, kind, message) {
                        Some(Received::Data(request)) => {
                            let mut sink = Sink {
                                link: &mut self.link,
                                tx: &mut self.tx,
                                clock: &self.clock,
                            };
                            process_request(&mut sink, backend, request, self.scratch).await;
                        }
                        Some(Received::Loopback { repeat, seq, data }) => {
                            let mut sink = Sink {
                                link: &mut self.link,
                                tx: &mut self.tx,
                                clock: &self.clock,
                            };
                            sink.loopback(repeat, seq, data, self.scratch).await;
                        }
                        None => {}
                    }
                }
                self.rx.consume();
            }
        }
        self.drain_transmit().await;
        self.link.poll_event()
    }

    /// Wait and handle forever, for devices that have nothing else to select on.
    pub async fn run<B: WireWeaverAsyncApiBackend>(&mut self, backend: &mut B) -> ! {
        loop {
            let ready = self.wait().await;
            if let Some(event) = self.handle(ready, backend).await {
                info!("{:?}", event);
            }
        }
    }

    /// Sink to send stream updates or deferred replies from outside of [handle](Self::handle),
    /// and scratch space to serialize them.
    pub fn sink(&mut self) -> (Sink<'_, 'a, Tx, C>, &mut [u8]) {
        (
            Sink {
                link: &mut self.link,
                tx: &mut self.tx,
                clock: &self.clock,
            },
            self.scratch,
        )
    }

    /// Send a data message (serialized `ww_client_server::Event`).
    pub async fn send(&mut self, message: &[u8]) -> Result<(), SendError> {
        self.sink().0.send_message(message).await
    }

    /// Tell the host that the device is going away (e.g., rebooting to perform a firmware update)
    /// and send it out right away.
    pub async fn disconnect(&mut self, reason: ww_link::DisconnectReason) {
        self.link.disconnect(reason);
        self.drain_transmit().await;
    }

    /// Write out everything the link core wants to send.
    async fn drain_transmit(&mut self) {
        loop {
            let now = self.clock.now();
            let Some(t) = self.link.poll_transmit(now, self.scratch) else {
                break;
            };
            let r = match t {
                Transmit::Message { kind, bytes } => self.tx.write_message(kind, bytes).await,
                Transmit::Flush => self.tx.flush().await,
            };
            if r.is_err() {
                warn!("write failed, transport down");
                self.link.on_transport_down();
                break;
            }
        }
    }
}

/// Writes data messages into the framer, obtained with [Server::sink] or passed to the backend.
pub struct Sink<'s, 'a, Tx, C> {
    link: &'s mut DeviceLink<'a>,
    tx: &'s mut Tx,
    clock: &'s C,
}

impl<Tx: MessageTx, C: Clock> Sink<'_, '_, Tx, C> {
    pub fn is_up(&self) -> bool {
        self.link.is_up()
    }

    /// Send a data message (serialized `ww_client_server::Event`). It goes out when a frame is full or
    /// after the accumulation window.
    pub async fn send_message(&mut self, message: &[u8]) -> Result<(), SendError> {
        self.link.check_send(message.len())?;
        match self
            .tx
            .write_message(DeviceLink::data_kind(), message)
            .await
        {
            Ok(()) => {
                self.link.on_data_written(self.clock.now());
                Ok(())
            }
            Err(_) => {
                warn!("write failed, transport down");
                self.link.on_transport_down();
                Err(SendError::Transport)
            }
        }
    }

    async fn loopback(&mut self, repeat: u32, seq: u32, data: &[u8], scratch: &mut [u8]) {
        for seq in crate::loopback_seqs(repeat, seq) {
            let Some((kind, bytes)) = crate::loopback_reply(seq, data, scratch) else {
                return;
            };
            if self.tx.write_message(kind, bytes).await.is_err() {
                self.link.on_transport_down();
                return;
            }
            self.link.on_data_written(self.clock.now());
        }
    }
}

impl<Tx: MessageTx, C: Clock> MessageSink for Sink<'_, '_, Tx, C> {
    async fn send(&mut self, message: &[u8]) -> Result<(), ()> {
        self.send_message(message).await.map_err(|e| {
            warn!("MessageSink::send failed: {:?}", e);
        })
    }
}

async fn process_request<B: WireWeaverAsyncApiBackend, Tx: MessageTx, C: Clock>(
    sink: &mut Sink<'_, '_, Tx, C>,
    backend: &mut B,
    request: &[u8],
    scratch: &mut [u8],
) {
    let reply = match backend.process_bytes(sink, request, scratch).await {
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
        _ = sink.send_message(reply).await;
    }
}

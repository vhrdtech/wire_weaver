//! Sans-IO device side of [ww_link]: link setup, version check, frame accumulation window, pings and
//! peer timeout. No IO, no timers, no allocations — the caller feeds received messages and time, and
//! drains messages to send.
//!
//! ```text
//!  medium ──► framer ──(kind, payload)──► handle_message ──► Received::Data ──► backend
//!                                         handle_timeout
//!  medium ◄── framer ◄──(kind, payload)── poll_transmit  ◄── (control replies, pings, flushes)
//!                                         poll_event     ──► LinkEvent::Up / Down
//! ```
//!
//! Data written by the application (backend replies, stream updates) goes straight into the framer,
//! bypassing the core; only [DeviceLink::check_send] and [DeviceLink::on_data_written] are needed for it.

use core::time::Duration;

use ww_link::{
    ApiHashPair, CompactVersion, DeviceInfo, DisconnectReason, FullVersion, Kind, Message,
    PEER_TIMEOUT_MS, PING_INTERVAL_MS,
};

use crate::fmt::{debug, error, info, warn};
use crate::time::Instant;

/// Deadlines this close to `now` are considered due, to avoid spinning on tiny sleeps.
const TIMER_TOLERANCE: Duration = Duration::from_micros(10);

/// Everything that is reported to the host in [DeviceInfo] and link timings.
#[derive(Clone)]
pub struct LinkConfig<'a> {
    /// User API crate name and version, the host checks it for compatibility.
    pub user_api_version: FullVersion<'a>,
    /// Hash of the user API, from generated `api_hash()`.
    pub api_hash: ApiHashPair<'a>,
    /// Usually `ww_client_server::COMPACT_VERSION`.
    pub api_model_version: CompactVersion,
    /// Data messages are accumulated into one frame for this long before the frame is sent.
    /// Host is told to use the same value.
    pub accumulation_time: Duration,
    /// Ping is sent if nothing else was sent for this long.
    pub ping_interval: Duration,
    /// Host is considered gone if nothing was received from it for this long.
    pub peer_timeout: Duration,
}

impl<'a> LinkConfig<'a> {
    /// Config with default timings: 1ms accumulation window, ping and peer timeout from [ww_link].
    pub fn new(
        user_api_version: FullVersion<'a>,
        api_hash: ApiHashPair<'a>,
        api_model_version: CompactVersion,
    ) -> Self {
        LinkConfig {
            user_api_version,
            api_hash,
            api_model_version,
            accumulation_time: Duration::from_millis(1),
            ping_interval: Duration::from_millis(PING_INTERVAL_MS),
            peer_timeout: Duration::from_millis(PEER_TIMEOUT_MS),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum Phase {
    /// Medium is not usable (e.g., USB cable disconnected or not configured by the host).
    Down,
    /// Medium is usable, waiting for a host to go through link setup.
    Idle,
    /// Link setup done, data flows.
    Up,
}

/// Link state changes, see [DeviceLink::poll_event].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum LinkEvent {
    /// A compatible host went through link setup.
    Up,
    /// Link is no longer up.
    Down(DownReason),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum DownReason {
    /// Host sent Disconnect.
    Disconnect(DisconnectReason),
    /// Nothing received from the host for [LinkConfig::peer_timeout].
    PeerTimeout,
    /// Host started link setup again without disconnecting first (e.g., host application crashed and restarted).
    NewSession,
    /// Medium went down or a write failed.
    Transport,
    /// [DeviceLink::disconnect] was called.
    Local(DisconnectReason),
}

/// What the application should act on, see [DeviceLink::handle_message].
#[derive(Debug)]
pub enum Received<'m> {
    /// `ww_client_server` request bytes for the backend.
    Data(&'m [u8]),
    /// Send `data` back `repeat` times, see [loopback_reply](crate::loopback_reply).
    Loopback {
        repeat: u32,
        seq: u32,
        data: &'m [u8],
    },
}

/// See [DeviceLink::poll_transmit].
#[derive(Debug, PartialEq, Eq)]
pub enum Transmit<'s> {
    /// Write this message into the framer.
    Message { kind: u8, bytes: &'s [u8] },
    /// Send the current frame now, even if not full. No-op when nothing is pending.
    Flush,
}

/// Why a message cannot be sent right now, see [DeviceLink::check_send].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum SendError {
    /// Link is not up, nobody would receive it.
    NotConnected,
    /// Longer than the host accepts.
    TooBig,
    /// Write failed, link is going down.
    Transport,
}

/// Control messages waiting for [DeviceLink::poll_transmit]. A handful of flags instead of a queue,
/// as the device only ever sends a few kinds of them.
#[derive(Default)]
struct Pending {
    nop: bool,
    device_info: bool,
    link_ready: bool,
    disconnect: Option<DisconnectReason>,
    ping: bool,
    flush: bool,
}

impl Pending {
    fn is_empty(&self) -> bool {
        !(self.nop
            || self.device_info
            || self.link_ready
            || self.disconnect.is_some()
            || self.ping
            || self.flush)
    }
}

pub struct DeviceLink<'a> {
    config: LinkConfig<'a>,
    /// Maximum message length this device can receive, reported to the host.
    max_message_len: u32,
    phase: Phase,
    remote_max_message_len: u32,
    pending: Pending,
    event: Option<LinkEvent>,

    last_rx_at: Option<Instant>,
    /// Set when data was written but not yet flushed, waiting for more messages to fill the frame.
    unflushed_since: Option<Instant>,
    next_ping_at: Option<Instant>,
}

impl<'a> DeviceLink<'a> {
    /// `max_message_len` is the longest message this device can receive (reported to the host),
    /// usually determined by the receive buffer size.
    pub fn new(config: LinkConfig<'a>, max_message_len: usize) -> Self {
        DeviceLink {
            config,
            max_message_len: max_message_len.try_into().unwrap_or(u32::MAX),
            phase: Phase::Down,
            remote_max_message_len: 0,
            pending: Pending::default(),
            event: None,
            last_rx_at: None,
            unflushed_since: None,
            next_ping_at: None,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn is_up(&self) -> bool {
        self.phase == Phase::Up
    }

    pub fn config(&self) -> &LinkConfig<'a> {
        &self.config
    }

    /// Maximum message length this device accepts, reported to the host.
    pub fn max_message_len(&self) -> usize {
        self.max_message_len as usize
    }

    /// Maximum message length the host accepts, valid when the link is up.
    pub fn remote_max_message_len(&self) -> usize {
        self.remote_max_message_len as usize
    }

    /// Medium became usable (e.g., USB configured by the host).
    pub fn on_transport_up(&mut self, now: Instant) {
        let _ = now;
        if self.phase == Phase::Down {
            debug!("transport up");
            self.phase = Phase::Idle;
        }
    }

    /// Medium went down or a write failed. Anything pending is dropped.
    pub fn on_transport_down(&mut self) {
        if self.phase != Phase::Down {
            debug!("transport down");
            self.go_idle(DownReason::Transport);
            self.phase = Phase::Down;
            self.pending = Pending::default();
        }
    }

    /// Feed one de-framed link message (framer `user_kind` and payload).
    pub fn handle_message<'m>(
        &mut self,
        now: Instant,
        kind: u8,
        payload: &'m [u8],
    ) -> Option<Received<'m>> {
        if self.phase == Phase::Down {
            warn!("message received while transport is down, ignoring");
            return None;
        }
        self.last_rx_at = Some(now);
        let msg = match Message::decode(kind, payload) {
            Ok(msg) => msg,
            Err(e) => {
                // unknown kind from a newer host or a leftover from a previous session
                warn!("ignoring link message: {:?}", e);
                return None;
            }
        };
        match msg {
            Message::Data { channel: 0, bytes } => {
                if self.phase != Phase::Up {
                    warn!("data before link is up, ignoring");
                    return None;
                }
                if bytes.is_empty() {
                    return None;
                }
                return Some(Received::Data(bytes));
            }
            Message::Data { channel, .. } => {
                warn!("ignoring data on unused channel {}", channel);
            }
            Message::Nop | Message::Ping => {}
            Message::GetDeviceInfo => {
                if self.phase == Phase::Up {
                    warn!("link setup while link is up, previous host session did not disconnect");
                    self.go_idle(DownReason::NewSession);
                }
                debug!("GetDeviceInfo");
                // Nop is flushed alone first: if USB data toggle bits are messed up after
                // re-connection, the first packet might get lost and this ensures it's not DeviceInfo.
                self.pending.nop = true;
                self.pending.device_info = true;
            }
            Message::LinkSetup(setup) => {
                // a host without generated client code works with the API dynamically, via introspection
                let dynamic_host = setup.host_user_version.crate_id.is_empty();
                let compatible = dynamic_host
                    || self
                        .config
                        .user_api_version
                        .is_protocol_compatible(&setup.host_user_version);
                if compatible {
                    info!("host connected: {:?}", setup.host_user_version);
                    self.remote_max_message_len = setup.host_max_message_len;
                    self.pending.link_ready = true;
                    if self.phase != Phase::Up {
                        self.phase = Phase::Up;
                        self.event = Some(LinkEvent::Up);
                        self.next_ping_at = Some(now + self.config.ping_interval);
                    }
                } else {
                    warn!(
                        "host with incompatible version tried to connect: {:?}",
                        setup.host_user_version
                    );
                    self.go_idle(DownReason::NewSession);
                    self.pending.disconnect = Some(DisconnectReason::IncompatibleVersion);
                }
            }
            Message::Disconnect(reason) => {
                if self.phase == Phase::Up {
                    info!("host disconnected: {:?}", reason);
                    self.go_idle(DownReason::Disconnect(reason));
                } else {
                    // leftover from a previous session
                    debug!("ignoring Disconnect while not connected");
                }
            }
            Message::Loopback { repeat, seq, data } => {
                if self.phase == Phase::Up {
                    return Some(Received::Loopback { repeat, seq, data });
                }
            }
            Message::GetStats => {
                // TODO: link statistics
            }
            Message::DeviceInfo(_) | Message::LinkReady | Message::Stats(_) => {
                warn!("ignoring host-only message");
            }
        }
        None
    }

    /// Feed when [Self::poll_timeout] deadline is reached. Calling it earlier is harmless.
    pub fn handle_timeout(&mut self, now: Instant) {
        if self.phase != Phase::Up {
            return;
        }
        if is_due(self.last_rx_at.map(|t| t + self.config.peer_timeout), now) {
            warn!("nothing received from host for too long, link down");
            self.go_idle(DownReason::PeerTimeout);
            return;
        }
        if is_due(
            self.unflushed_since
                .map(|t| t + self.config.accumulation_time),
            now,
        ) {
            self.pending.flush = true;
            self.unflushed_since = None;
        } else if is_due(self.next_ping_at, now) {
            self.pending.ping = true;
            self.next_ping_at = None; // restarted on flush
        }
    }

    /// Earliest instant at which [Self::handle_timeout] should be called, None if nothing is scheduled.
    ///
    /// Only valid after [Self::poll_transmit] returned None.
    pub fn poll_timeout(&self) -> Option<Instant> {
        if self.phase != Phase::Up {
            return None;
        }
        [
            self.last_rx_at.map(|t| t + self.config.peer_timeout),
            self.unflushed_since
                .map(|t| t + self.config.accumulation_time),
            self.next_ping_at,
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// Next control message to write into the framer, encoded into `scratch`, or a request to flush.
    /// Call until None after every input.
    pub fn poll_transmit<'s>(&mut self, now: Instant, scratch: &'s mut [u8]) -> Option<Transmit<'s>>
    where
        'a: 's,
    {
        let msg = if self.pending.nop {
            self.pending.nop = false;
            Message::Nop
        } else if self.pending.flush {
            self.pending.flush = false;
            self.unflushed_since = None;
            if self.phase == Phase::Up {
                self.next_ping_at = Some(now + self.config.ping_interval);
            }
            return Some(Transmit::Flush);
        } else if self.pending.device_info {
            self.pending.device_info = false;
            Message::DeviceInfo(DeviceInfo {
                dev_link_version: ww_link::LINK_VERSION,
                api_model_version: self.config.api_model_version,
                user_api_version: self.config.user_api_version,
                hash: self.config.api_hash,
                dev_max_message_len: self.max_message_len,
                packet_accumulation_time_us: self
                    .config
                    .accumulation_time
                    .as_micros()
                    .try_into()
                    .unwrap_or(u16::MAX),
            })
        } else if self.pending.link_ready {
            self.pending.link_ready = false;
            Message::LinkReady
        } else if let Some(reason) = self.pending.disconnect.take() {
            Message::Disconnect(reason)
        } else if self.pending.ping {
            self.pending.ping = false;
            Message::Ping
        } else {
            return None;
        };
        // control messages are flushed right away
        self.pending.flush = true;
        match msg.encode(scratch) {
            Ok((kind, bytes)) => Some(Transmit::Message { kind, bytes }),
            Err(_) => {
                error!("scratch buffer is too small for {:?}", msg.kind());
                None
            }
        }
    }

    /// Whether [Self::poll_transmit] would return something.
    pub fn wants_transmit(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Link state change since the last call, if any. Only the latest one is kept.
    pub fn poll_event(&mut self) -> Option<LinkEvent> {
        self.event.take()
    }

    /// Check before writing a data message of `len` bytes into the framer.
    pub fn check_send(&self, len: usize) -> Result<(), SendError> {
        if self.phase != Phase::Up {
            return Err(SendError::NotConnected);
        }
        if len > self.remote_max_message_len as usize {
            return Err(SendError::TooBig);
        }
        Ok(())
    }

    /// A data message was written into the framer, starts the accumulation window.
    pub fn on_data_written(&mut self, now: Instant) {
        if self.unflushed_since.is_none() {
            self.unflushed_since = Some(now);
        }
    }

    /// Framer kind to use for data messages.
    pub const fn data_kind() -> u8 {
        Kind::Data0 as u8
    }

    /// Tell the host that the device is going away (e.g., rebooting to perform a firmware update).
    /// Disconnect is sent on the next [Self::poll_transmit].
    pub fn disconnect(&mut self, reason: DisconnectReason) {
        if self.phase == Phase::Up {
            self.go_idle(DownReason::Local(reason));
            self.pending.disconnect = Some(reason);
        }
    }

    fn go_idle(&mut self, reason: DownReason) {
        if self.phase == Phase::Up {
            self.event = Some(LinkEvent::Down(reason));
        }
        self.phase = Phase::Idle;
        self.remote_max_message_len = 0;
        self.last_rx_at = None;
        self.unflushed_since = None;
        self.next_ping_at = None;
    }
}

fn is_due(deadline: Option<Instant>, now: Instant) -> bool {
    deadline.is_some_and(|d| d.saturating_duration_since(now) < TIMER_TOLERANCE)
}

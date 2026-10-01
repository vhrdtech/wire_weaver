//! RTT (SEGGER Real-Time Transfer) as a medium, on top of [rtt_target] channels: an up channel
//! carries device to host bytes, a down channel host to device. Both are ring buffers in RAM that a
//! debug probe reads and writes over SWD / JTAG, next to (or instead of) `defmt` logging.
//!
//! RTT has no notion of a connection and no way to wake the device when the host writes, so the
//! down channel is polled at [RttConfig::poll_interval], and a host that is gone is detected by
//! the up channel staying full for [RttConfig::write_timeout] (plus the link's own peer timeout).
//!
//! Create the channels with `rtt_target::rtt_init!` and wrap them in [RttSink] / [RttSource], then
//! use them with [StreamTx] / [StreamRx], or all at once with
//! [rtt_server]. Channels must be in `NoBlockSkip` or `NoBlockTrim` mode: `BlockIfFull` spins
//! forever without a host. With `NoBlockSkip` the up channel must be at least as large as the
//! tx chunk buffer, otherwise a chunk that does not fit in one go is never written.

use core::time::Duration;

use rtt_target::{DownChannel, UpChannel};
use ww_link::{RttChecksum, RttHead, RttTail};

use crate::fmt::warn;
use crate::server::Clock;
use crate::transport::{StreamRx, StreamSink, StreamSource, StreamTx, stream_overhead};
use crate::{LinkConfig, Server};

/// Timings of [RttSink] and [RttSource].
#[derive(Copy, Clone, Debug)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct RttConfig {
    /// How often the down channel is checked for new bytes and the up channel for free space.
    /// Bounds the request latency and how much CPU is spent on polling.
    pub poll_interval: Duration,
    /// Up channel is full for this long: the host is not reading, the link goes down.
    pub write_timeout: Duration,
}

impl Default for RttConfig {
    fn default() -> Self {
        RttConfig {
            poll_interval: Duration::from_millis(1),
            write_timeout: Duration::from_millis(500),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum RttError {
    /// Up channel was full for [RttConfig::write_timeout], nobody reads it.
    WriteTimeout,
}

/// [StreamSink] over an RTT up channel.
pub struct RttSink<C> {
    channel: UpChannel,
    clock: C,
    config: RttConfig,
}

impl<C: Clock> RttSink<C> {
    pub fn new(channel: UpChannel, clock: C, config: RttConfig) -> Self {
        RttSink {
            channel,
            clock,
            config,
        }
    }

    pub fn channel_mut(&mut self) -> &mut UpChannel {
        &mut self.channel
    }
}

impl<C: Clock> StreamSink for RttSink<C> {
    type Error = RttError;

    async fn write_all(&mut self, mut bytes: &[u8]) -> Result<(), RttError> {
        let mut full_since = None;
        while !bytes.is_empty() {
            let written = self.channel.write(bytes);
            bytes = &bytes[written..];
            if bytes.is_empty() {
                break;
            }
            let now = self.clock.now();
            if written > 0 {
                full_since = None;
            }
            let since = *full_since.get_or_insert(now);
            if now.saturating_duration_since(since) >= self.config.write_timeout {
                warn!("rtt up channel is full, host is not reading");
                return Err(RttError::WriteTimeout);
            }
            self.clock.wait_until(now + self.config.poll_interval).await;
        }
        Ok(())
    }
}

/// [StreamSource] over an RTT down channel.
pub struct RttSource<C> {
    channel: DownChannel,
    clock: C,
    config: RttConfig,
}

impl<C: Clock> RttSource<C> {
    pub fn new(channel: DownChannel, clock: C, config: RttConfig) -> Self {
        RttSource {
            channel,
            clock,
            config,
        }
    }

    pub fn channel_mut(&mut self) -> &mut DownChannel {
        &mut self.channel
    }
}

impl<C: Clock> StreamSource for RttSource<C> {
    type Error = RttError;

    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, RttError> {
        loop {
            let len = self.channel.read(buf);
            if len > 0 {
                return Ok(len);
            }
            let now = self.clock.now();
            self.clock.wait_until(now + self.config.poll_interval).await;
        }
    }

    async fn wait_connected(&mut self) {
        // a probe can attach and detach at any time without the device knowing
    }
}

/// [StreamTx] over an RTT up channel, framer configuration from [ww_link].
pub type RttTx<'a, C> = StreamTx<'a, RttSink<C>, RttHead, RttChecksum, RttTail>;
/// [StreamRx] over an RTT down channel, framer configuration from [ww_link].
pub type RttRx<'a, C> = StreamRx<'a, RttSource<C>, RttHead, RttChecksum, RttTail>;
/// WireWeaver server over RTT, see [Server] on how to use it.
pub type RttServer<'a, C, M = ()> = Server<'a, RttTx<'a, C>, RttRx<'a, C>, C, M>;

/// Framer overhead per message on RTT, see [RttBuffers].
pub const RTT_OVERHEAD: usize = stream_overhead::<RttHead, RttChecksum, RttTail>();

/// Buffers used by [RttServer], `MAX_MESSAGE_LEN` is the longest message the device accepts and
/// the longest reply it can serialize, reported to the host exactly as is.
///
/// Takes `4 * MAX_MESSAGE_LEN + 2 * RTT_OVERHEAD` bytes, in addition to the RTT channel buffers
/// themselves, which are allocated by `rtt_init!`.
#[repr(C)]
pub struct RttBuffers<const MAX_MESSAGE_LEN: usize> {
    /// Received bytes are cut into messages here
    rx: [u8; MAX_MESSAGE_LEN],
    rx_overhead: [u8; RTT_OVERHEAD],
    /// Messages are packed into chunks here before going into the up channel
    tx: [u8; MAX_MESSAGE_LEN],
    tx_overhead: [u8; RTT_OVERHEAD],
    /// Used to serialize replies and link messages
    scratch: [u8; MAX_MESSAGE_LEN],
    /// Used to serialize events sent from handlers and through `server.sink()`
    event_scratch: [u8; MAX_MESSAGE_LEN],
}

impl<const MAX_MESSAGE_LEN: usize> RttBuffers<MAX_MESSAGE_LEN> {
    pub const fn new() -> Self {
        RttBuffers {
            rx: [0u8; MAX_MESSAGE_LEN],
            rx_overhead: [0u8; RTT_OVERHEAD],
            tx: [0u8; MAX_MESSAGE_LEN],
            tx_overhead: [0u8; RTT_OVERHEAD],
            scratch: [0u8; MAX_MESSAGE_LEN],
            event_scratch: [0u8; MAX_MESSAGE_LEN],
        }
    }
}

impl<const MAX_MESSAGE_LEN: usize> Default for RttBuffers<MAX_MESSAGE_LEN> {
    fn default() -> Self {
        Self::new()
    }
}

/// Server over an RTT up / down channel pair from `rtt_target::rtt_init!`, e.g.:
/// ```ignore
/// let channels = rtt_init! {
///     up: { 0: { size: 1024, name: "defmt" } 1: { size: 1024, name: "ww_up" } }
///     down: { 0: { size: 1024, name: "ww_down" } }
/// };
/// rtt_target::set_defmt_channel(channels.up.0);
/// let buffers = RTT_BUFFERS.init(RttBuffers::new());
/// let mut server = rtt_server(link_config, channels.up.1, channels.down.0, EmbassyClock, RttConfig::default(), buffers);
/// server.run(&mut state).await;
/// ```
pub fn rtt_server<'a, const MAX_MESSAGE_LEN: usize, C: Clock + Clone>(
    link_config: LinkConfig<'a>,
    up: UpChannel,
    down: DownChannel,
    clock: C,
    config: RttConfig,
    buffers: &'a mut RttBuffers<MAX_MESSAGE_LEN>,
) -> RttServer<'a, C> {
    let (tx_buf, rx_buf, scratch, event_scratch) = buffers.split();
    Server::new(
        link_config,
        StreamTx::new(RttSink::new(up, clock.clone(), config), tx_buf),
        StreamRx::new(RttSource::new(down, clock.clone(), config), rx_buf),
        clock,
        scratch,
        event_scratch,
    )
}

impl<const MAX_MESSAGE_LEN: usize> RttBuffers<MAX_MESSAGE_LEN> {
    /// Contiguous `MAX_MESSAGE_LEN + RTT_OVERHEAD` tx and rx buffers, and the scratch buffers.
    #[allow(clippy::type_complexity)]
    fn split(&mut self) -> (&mut [u8], &mut [u8], &mut [u8], &mut [u8]) {
        const {
            assert!(
                size_of::<Self>() == 4 * MAX_MESSAGE_LEN + 2 * RTT_OVERHEAD,
                "RttBuffers must have no padding"
            )
        };
        let (tx, rx) = (self.tx.as_mut_ptr(), self.rx.as_mut_ptr());
        // SAFETY: repr(C) struct of u8 arrays: alignment 1 and no padding (checked above), so `rx`
        // is followed by `rx_overhead` and `tx` by `tx_overhead`, both are contiguous, initialized
        // bytes borrowed mutably through self; the ranges do not overlap with each other or the scratch buffers.
        unsafe {
            (
                core::slice::from_raw_parts_mut(tx, MAX_MESSAGE_LEN + RTT_OVERHEAD),
                core::slice::from_raw_parts_mut(rx, MAX_MESSAGE_LEN + RTT_OVERHEAD),
                &mut self.scratch,
                &mut self.event_scratch,
            )
        }
    }
}

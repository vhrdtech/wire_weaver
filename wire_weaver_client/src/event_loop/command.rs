use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use wire_weaver::DisconnectReason;
use ww_client_server::PathKindOwned;
use ww_version::FullVersionOwned;

use crate::client::observe::ObserveRouteHook;
use crate::client::positioned::Shared;
use crate::client::prepared_stream_open::{StreamOpenResult, is_open};
use crate::client::sink::SinkLink;
use crate::{
    SeqTy,
    device_info::ConnectionInfo,
    event_loop::rx_dispatcher::{ResponseSender, StreamUpdateSender},
    tracing::TraceEvent,
};
use ww_stream::StreamCursor;

/// Command for the transport event loop host (USB host, WebSocket client, UDP client).
/// Generated client code uses [Commander](crate::Commander), which sends out Command's.
pub enum Command {
    /// Connect to a device identified by a provided handle.
    /// On success, send a message through connected_tx.
    /// On failure, send a message through failed_tx.
    Connect {
        /// Interface specific handle or device to connect to (e.g., nusb::DeviceInfo for USB)
        handle: Box<dyn Any + Send>,
        client_version: Box<FullVersionOwned>,
        /// Largest request seq number to use, see [ClientConfig::max_seq](crate::ClientConfig::max_seq).
        max_seq: SeqTy,
        /// How long link setup may take, [ClientConfig::link_setup_timeout](crate::ClientConfig::link_setup_timeout).
        /// None - transport's default. Milliseconds, to keep [Command] small.
        link_setup_timeout_ms: Option<std::num::NonZeroU32>,
        /// Where the link is recorded, [ClientConfig::tap](crate::ClientConfig::tap); None - the default recorder.
        tap: Option<ww_tap::Recorder>,
        /// Who this client is, sent in link setup, [ClientConfig::client_meta](crate::ClientConfig::client_meta).
        client_meta: Option<Box<ww_link::ClientMetaOwned>>,
        /// Connection status sender.
        connected_tx: Option<oneshot::Sender<ConnectionInfo>>,
        /// Before exiting, event loop will return it's command receiver through this channel.
        /// Can be used to re-connect without dropping all command senders client code uses.
        failed_tx: Option<oneshot::Sender<EventLoopResidual>>,
    },

    /// Send ww_client_server Request to a remote device
    SendMessage {
        bytes: Vec<u8>,
        /// If None - the message will be sent with seq = 0
        /// TODO: Timeout per each update or in total for multipart?
        done_tx: Option<(ResponseSender, Duration)>,
    },
    /// Open a counted or reliable stream: send the `StreamOpen` request in `bytes` like [Command::SendMessage], and
    /// route the stream's events to `hook` once the device answered that it is open.
    OpenStream {
        bytes: Vec<u8>,
        done_tx: ResponseSender,
        timeout: Duration,
        hook: Box<StreamOpenHook>,
    },
    /// Subscribe to a stream or property change
    OnStreamEvent {
        path_kind: Box<PathKindOwned>,
        stream_event_tx: StreamUpdateSender,
        /// Notified once the dispatcher routes events on this path to `stream_event_tx`, events received before
        /// that are not delivered.
        registered_tx: oneshot::Sender<()>,
        /// A sink's link, told when the connection goes (`keep_streams` or not)
        sink: Option<Arc<SinkLink>>,
    },

    /// Route `PropertyUpdate` events of a trait level to its observers (CLT-28), on this connection.
    ObserveRoute { hook: Box<ObserveRouteHook> },

    /// Send a request serialized elsewhere as it is (the ww hub forwards its clients' requests, SRV-29): `bytes` is a
    /// whole `ww_client_server::Request` whose seq is the [UVlq32Backfill](wire_weaver::shrink_wrap::UVlq32Backfill)
    /// placeholder. With a `tag`, the request gets a seq and every event answering it goes to the
    /// [Command::ForwardSink] as [Forwarded::Reply]; without one it goes out with seq 0, unanswered. Unlike
    /// [Command::SendMessage], nothing is decoded or rewritten (no `GlobalFull` retry, API-20: the request's own
    /// client does that).
    Forward {
        bytes: Vec<u8>,
        tag: Option<u64>,
        /// The seq is freed after this long without the last answer (and the [Forwarded::Failed] goes to the sink)
        timeout: Duration,
    },
    /// Where [Command::Forward] answers, events nobody asked for (seq 0: stream data, property updates) and loopback
    /// echoes go, raw, instead of to this client's own streams and observers.
    ForwardSink {
        tx: mpsc::UnboundedSender<Forwarded>,
    },
    /// Send a link `Loopback` message as it is (a client's `ww usb-loopback` through the hub); the echoes go to the
    /// [Command::ForwardSink]. Ignored while a [Command::LoopbackTest] of this client runs.
    ForwardLoopback {
        repeat: u32,
        seq: u32,
        data: Vec<u8>,
    },

    /// Close a device connection and stop the worker task. All outstanding requests will return with Error,
    /// and streams will stop. Use when shutting down the whole application.
    DisconnectAndExit {
        disconnected_tx: Option<oneshot::Sender<()>>,
        reason: DisconnectReason,
    },

    /// Complete outstanding requests (but ignore new ones)? Then, close the device connection but keep the worker task running.
    /// This allows all the outstanding streams to still be valid and continue upon reconnection.
    /// Alternatively, it's also possible to connect to a different device, without other parts noticing.
    DisconnectKeepStreams {
        disconnected_tx: Option<oneshot::Sender<()>>,
        reason: DisconnectReason,
    },

    /// All incoming messages from a device and all outgoing commands will be sent to this channel.
    /// Multiple tracers could be installed.
    RegisterTracer {
        trace_event_tx: mpsc::UnboundedSender<TraceEvent>,
    },

    // RecycleBuffer(Vec<u8>),
    // GetStats,
    /// Link loopback and speed test: round trips, host → device and device → host, `test_duration` each.
    /// Other requests are held until it is done. Progress and results go to `progress_tx`, which is dropped
    /// when the test is over.
    LoopbackTest {
        test_duration: Duration,
        /// Size of each link message (8 bytes of header and data), capped to what both sides accept, which
        /// is also the default.
        packet_size: Option<usize>,
        /// Timeouts and flow control
        params: Box<LoopbackParams>,
        progress_tx: mpsc::UnboundedSender<TestProgress>,
    },
}

/// What the event loop hands to a [Command::ForwardSink].
#[derive(Debug)]
pub enum Forwarded {
    /// An event answering the [Command::Forward] with `tag`, with the seq the device used. `last`: the request is
    /// answered (a value, `Written`, an error or a sideband), the events before it carried stream data under its seq
    /// (introspection).
    Reply {
        tag: u64,
        event: Vec<u8>,
        last: bool,
    },
    /// An event with seq 0: stream data or sideband, a stream item, a property update.
    Event(Vec<u8>),
    /// Payload of a `Loopback` link message from the device.
    Loopback(Vec<u8>),
    /// The [Command::Forward] with `tag` got no (last) answer: not sent (too long for the device, malformed), timed
    /// out or the link went down.
    Failed { tag: u64, error: crate::Error },
}

/// Where the events of a counted or reliable stream go once it is open, see [Command::OpenStream].
#[doc(hidden)]
pub struct StreamOpenHook {
    pub(crate) shared: Arc<Shared>,
    /// Resumed from here
    pub(crate) resume: Option<StreamCursor>,
    /// Whether the answer says the stream is open (the stream function returned `Ok`)
    pub(crate) is_open: fn(&[u8]) -> bool,
}

impl Debug for StreamOpenHook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StreamOpenHook")
            .field("path", &self.shared.lock().path)
            .field("resume", &self.resume)
            .finish()
    }
}

impl StreamOpenHook {
    pub(crate) fn new<R: StreamOpenResult>(
        shared: Arc<Shared>,
        resume: Option<StreamCursor>,
    ) -> Self {
        StreamOpenHook {
            shared,
            resume,
            is_open: is_open::<R>,
        }
    }
}

pub struct EventLoopResidual {
    pub cmd_rx: mpsc::Receiver<Command>,
    // dispatcher_tx: mpsc::
    /// If event loop spawned, but failed to connect, keep this around for eventual re-connect.
    /// (Only if not exiting on error)
    pub connected_tx: Option<oneshot::Sender<ConnectionInfo>>,
    pub result: anyhow::Result<EventLoopExitReason>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum EventLoopExitReason {
    CommanderDropped,
    DisconnectCommand,
    DisconnectKeepStreamsCommand,
    DisconnectFromDevice,
}

const _: () = {
    assert!(size_of::<Command>() <= 80); // was 56, then 64 until Connect took the tap, 72 until it took the client meta
};

/// Tunables of [Command::LoopbackTest], [Default] has the values `ww usb-loopback` uses unless told otherwise.
/// Counts of 0 are taken as 1, [LoopbackParams::rx_batch] at least 2.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoopbackParams {
    /// An expected echo (round trip, marker or batch reply) that did not come back for this long is lost.
    pub echo_timeout: Duration,
    /// Give up after this many echo timeouts in a row: the device does not answer at all.
    pub timeouts_in_a_row: u32,
    /// Host → device: messages between two echoed markers.
    pub marker_every: u32,
    /// Host → device: markers in flight at most, the host waits for echoes beyond that.
    pub markers_in_flight: u32,
    /// Device → host: messages the device sends for one request (`repeat`).
    pub rx_batch: u32,
    /// Device → host: requests in flight at most.
    pub batches_in_flight: u32,
}

impl Default for LoopbackParams {
    fn default() -> Self {
        LoopbackParams {
            echo_timeout: Duration::from_millis(500),
            timeouts_in_a_row: 3,
            marker_every: 32,
            markers_in_flight: 2,
            rx_batch: 64,
            batches_in_flight: 2,
        }
    }
}

impl LoopbackParams {
    /// Counts the test can work with: `repeat = 1` keeps the request's seq, so a batch is 2 at least.
    pub(crate) fn normalized(self) -> Self {
        LoopbackParams {
            echo_timeout: self.echo_timeout,
            timeouts_in_a_row: self.timeouts_in_a_row.max(1),
            marker_every: self.marker_every.max(1),
            markers_in_flight: self.markers_in_flight.max(1),
            rx_batch: self.rx_batch.max(2),
            batches_in_flight: self.batches_in_flight.max(1),
        }
    }
}

/// Progress of [Command::LoopbackTest]. Stages are `loopback`, `tx_speed` (host → device) and `rx_speed`
/// (device → host).
#[derive(Debug)]
pub enum TestProgress {
    TestStarted(&'static str),
    /// Stage name and how much of its time is over, 0.0 to 1.0
    Completion(&'static str, f32),
    TestCompleted(&'static str),
    /// The test is aborted.
    FatalError(String),
    /// Result of the `loopback` stage.
    LoopbackReport {
        /// Round trips started
        tx_count: u64,
        per_s: f32,
        lost_count: u64,
        data_corrupted_count: u64,
    },
    /// Result of the `tx_speed` and `rx_speed` stages.
    SpeedReport {
        name: &'static str,
        /// Messages sent (`tx_speed`) or received (`rx_speed`)
        count: u64,
        per_s: f32,
        /// Link message bytes (header and data), without framing overhead
        bytes_per_s: f32,
        /// `rx_speed` only: replies missing in the sequence
        lost_count: u64,
        /// `rx_speed` only
        data_corrupted_count: u64,
    },
}

impl Command {
    pub fn disconnect_and_exit(reason: DisconnectReason) -> (Self, oneshot::Receiver<()>) {
        let (tx, rx) = oneshot::channel();
        let cmd = Command::DisconnectAndExit {
            disconnected_tx: Some(tx),
            reason,
        };
        (cmd, rx)
    }
}

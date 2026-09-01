use std::any::Any;
use std::fmt::Debug;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use wire_weaver::DisconnectReason;
use ww_client_server::PathKindOwned;
use ww_version::FullVersionOwned;

use crate::{
    device_info::ConnectionInfo,
    event_loop::rx_dispatcher::{ResponseSender, StreamUpdateSender},
    tracing::tracing::TraceEvent,
};

/// Command for the transport event loop host (USB host, WebSocket client, UDP client).
/// Generated client code uses [CommandSender](CommandSender), which sends out Command's.
pub enum Command {
    /// Connect to a device identified by a provided handle.
    /// On success, send a message through connected_tx.
    /// On failure, send a message through failed_tx.
    Connect {
        /// Interface specific handle or device to connect to (e.g., nusb::DeviceInfo for USB)
        handle: Box<dyn Any + Send>,
        client_version: Box<FullVersionOwned>,
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
    /// Subscribe to a stream or property change
    OnStreamEvent {
        path_kind: Box<PathKindOwned>,
        stream_event_tx: StreamUpdateSender,
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
    LoopbackTest {
        test_duration: Duration,
        packet_size: Option<usize>,
        progress_tx: mpsc::UnboundedSender<TestProgress>,
    },
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
    assert!(size_of::<Command>() <= 64); // was 56
};

#[derive(Debug)]
pub enum TestProgress {
    TestStarted(&'static str),
    Completion(&'static str, f32),
    TestCompleted(&'static str),
    FatalError(String),
    LoopbackReport {
        tx_count: u64,
        per_s: f32,
        lost_count: u64,
        data_corrupted_count: u64,
    },
    SpeedReport {
        name: &'static str,
        count: u64,
        per_s: f32,
        bytes_per_s: f32,
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

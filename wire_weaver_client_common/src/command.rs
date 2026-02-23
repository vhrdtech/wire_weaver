use crate::{DeviceFilter, Error, OnError};
use std::fmt::{Debug, Formatter};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use ww_version::{FullVersionOwned, VersionOwned};

/// Command for the transport event loop host (USB host, WebSocket client, UDP client).
/// Generated client code uses [CommandSender](CommandSender), which sends out Command's.
pub enum Command {
    /// Try to connect to / open a device with the specified filter.
    Connect {
        filter: DeviceFilter,
        client_version: FullVersionOwned,
        // TODO: supported_use_protocols: Vec<FullVersion<'static>> and keep only the one in common
        on_error: OnError,
        connected_tx: Option<oneshot::Sender<Result<DeviceInfoBundle, Error>>>,
    },
    /// All incoming messages from a device and all outgoing commands will be sent to this channel.
    RegisterTracer {
        trace_event_tx: mpsc::UnboundedSender<crate::tracing::TraceEvent>,
    },

    /// Complete outstanding requests (but ignore new ones)? Then, close the device connection but keep the worker task running.
    /// This allows all the outstanding streams to still be valid and continue upon reconnection.
    /// Alternatively, it's also possible to connect to a different device, without other parts noticing.
    DisconnectKeepStreams {
        disconnected_tx: Option<oneshot::Sender<()>>,
    },

    /// Close a device connection and stop the worker task. All outstanding requests will return with Error,
    /// and streams will stop. Use when shutting down the whole application.
    DisconnectAndExit {
        disconnected_tx: Option<oneshot::Sender<()>>,
    },

    SendMessage {
        bytes: Vec<u8>,
    },
    // RecycleBuffer(Vec<u8>),
    // GetStats,
    LoopbackTest {
        test_duration: Duration,
        packet_size: Option<usize>,
        progress_tx: mpsc::UnboundedSender<TestProgress>,
    },
}

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
    pub fn disconnect_and_exit() -> (Self, oneshot::Receiver<()>) {
        let (tx, rx) = oneshot::channel();
        let cmd = Command::DisconnectAndExit {
            disconnected_tx: Some(tx),
        };
        (cmd, rx)
    }
}

#[derive(Clone, Debug)]
pub struct DeviceInfoBundle {
    /// Link carries API model messages.
    pub link_version: FullVersionOwned,
    /// Maximum message size supported by the device.
    pub max_message_size: usize,
    /// API model defines what operations can be performed (call, write, etc.).
    pub api_model_version: FullVersionOwned,
    /// User-defined API carried by API model.
    pub user_api_version: FullVersionOwned,
    pub user_api_signature: UserApiSignature,
}

/// First 8 bytes for SHA256 of ww_self bytes without doc comments
#[derive(Clone, Default)]
pub struct UserApiSignature(pub Vec<u8>);

impl DeviceInfoBundle {
    pub fn empty() -> Self {
        DeviceInfoBundle {
            link_version: FullVersionOwned::new("".into(), VersionOwned::new(0, 0, 0)),
            max_message_size: 0,
            api_model_version: FullVersionOwned::new("".into(), VersionOwned::new(0, 0, 0)),
            user_api_version: FullVersionOwned::new("".into(), VersionOwned::new(0, 0, 0)),
            user_api_signature: Default::default(),
        }
    }
}

impl Debug for UserApiSignature {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", hex::encode(&self.0))
    }
}

impl From<Vec<u8>> for UserApiSignature {
    fn from(hash: Vec<u8>) -> Self {
        UserApiSignature(hash)
    }
}

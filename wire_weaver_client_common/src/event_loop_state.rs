use crate::tracing::TraceEvent;
use crate::{DeviceInfoBundle, Error, OnError};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use ww_version::FullVersionOwned;

pub struct CommonState {
    pub exit_on_error: bool,
    pub link_up: bool,
    pub connected_tx: Option<oneshot::Sender<Result<DeviceInfoBundle, Error>>>,
    pub packet_started_instant: Option<Instant>,
    pub last_rx_ping_instant: Option<Instant>,
    pub packet_accumulation_time: Duration,
    pub client_version: Option<FullVersionOwned>,
    // pub remote_version: Option<FullVersionOwned>,
    // pub remote_max_message_size: Option<usize>,
    // pub remote_link_version: Option<CompactVersion>,
    // pub remote_api_model_version: Option<CompactVersion>,
    pub device_info: Option<DeviceInfoBundle>,
    pub tracers: Vec<mpsc::UnboundedSender<TraceEvent>>,
}

impl Default for CommonState {
    fn default() -> Self {
        CommonState {
            exit_on_error: true,
            link_up: false,
            connected_tx: None,
            packet_started_instant: None,
            last_rx_ping_instant: None,
            packet_accumulation_time: Duration::from_millis(1),
            client_version: None,
            device_info: None,
            tracers: vec![],
        }
    }
}

impl CommonState {
    pub fn on_disconnect(&mut self) {
        self.link_up = false;
        self.packet_started_instant = None;
        self.connected_tx = None;
    }

    pub fn on_connect(
        &mut self,
        on_error: OnError,
        connected_tx: Option<oneshot::Sender<Result<DeviceInfoBundle, Error>>>,
        client_version: FullVersionOwned,
    ) {
        self.exit_on_error = on_error != OnError::KeepRetrying;
        self.connected_tx = connected_tx;
        self.client_version = Some(client_version);
    }

    pub fn on_link_up(&mut self) {
        self.link_up = true;
        self.tracers.retain_mut(|tx| {
            tx.send(TraceEvent::Connected {
                info: Box::new(crate::tracing::ConnectionInfo {}),
            })
            .is_ok()
        });
    }

    pub fn trace_request(&mut self, bytes: &[u8]) {
        self.tracers.retain_mut(|tx| {
            tx.send(TraceEvent::Request {
                bytes: bytes.to_vec(),
            })
            .is_ok()
        });
    }

    pub fn trace_event(&mut self, bytes: &[u8]) {
        self.tracers.retain_mut(|tx| {
            tx.send(TraceEvent::Event {
                bytes: bytes.to_vec(),
            })
            .is_ok()
        });
    }

    pub fn trace_disconnect(&mut self, reason: &str, keep_streams: bool) {
        self.tracers.retain_mut(|tx| {
            tx.send(TraceEvent::Disconnected {
                reason: reason.to_string(),
                keep_streams,
            })
            .is_ok()
        });
    }

    pub fn trace_error(&mut self, reason: String) {
        self.tracers.retain_mut(|tx| {
            tx.send(TraceEvent::Error {
                reason: reason.clone(),
            })
            .is_ok()
        });
    }
}

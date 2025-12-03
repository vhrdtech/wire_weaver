use crate::{DEFAULT_REQUEST_TIMEOUT, Error, OnError, SeqTy, StreamEvent};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};
use wire_weaver::prelude::UNib32;
use ww_client_server::StreamSidebandEvent;
use ww_version::FullVersionOwned;

pub type ResponseSender = oneshot::Sender<Result<Vec<u8>, Error>>;

pub type StreamUpdateSender = mpsc::UnboundedSender<StreamEvent>;

pub struct CommonState {
    pub exit_on_error: bool,
    pub request_id: SeqTy,
    // user_protocol: ProtocolInfo,
    // conn_state: Arc<RwLock<ConnectionInfo>>,
    pub connected_tx: Option<oneshot::Sender<Result<(), Error>>>,
    pub response_map: HashMap<SeqTy, (ResponseSender, Instant)>,
    pub stream_handlers: HashMap<Vec<UNib32>, StreamUpdateSender>, // TODO: multiple subscribers
    pub link_setup_done: bool,
    pub packet_started_instant: Option<Instant>,
    pub last_rx_ping_instant: Option<Instant>,
    pub packet_accumulation_time: Duration,
    pub user_protocol_version: Option<FullVersionOwned>,
}

impl Default for CommonState {
    fn default() -> Self {
        CommonState {
            exit_on_error: true,
            request_id: 1,
            connected_tx: None,
            response_map: Default::default(),
            stream_handlers: Default::default(),
            link_setup_done: false,
            packet_started_instant: None,
            last_rx_ping_instant: None,
            packet_accumulation_time: Duration::from_millis(1),
            user_protocol_version: None,
        }
    }
}

impl CommonState {
    pub fn increment_request_id(&mut self) {
        let mut iterations_left = SeqTy::MAX as usize;
        while self.response_map.contains_key(&self.request_id) && iterations_left > 0 {
            self.request_id = self.request_id.wrapping_add(1);
            if self.request_id == 0 {
                self.request_id += 1;
            }
            iterations_left -= 1;
        }
    }

    pub fn register_prune_next_seq(
        &mut self,
        timeout: Option<Duration>,
        done_tx: Option<ResponseSender>,
    ) -> SeqTy {
        if let Some(done_tx) = done_tx {
            let timeout = timeout.unwrap_or(DEFAULT_REQUEST_TIMEOUT);
            let prune_at = Instant::now() + timeout;
            self.response_map
                .insert(self.request_id, (done_tx, prune_at));
            let seq = self.request_id;
            self.increment_request_id();
            seq
        } else {
            0
        }
    }

    pub fn cancel_all_requests(&mut self) {
        for (_request_id, (tx, _)) in self.response_map.drain() {
            let _ = tx.send(Err(Error::Disconnected));
        }
    }

    pub fn cancel_all_streams(&mut self) {
        for (_path, tx) in self.stream_handlers.drain() {
            let _ = tx.send(StreamEvent::Sideband(StreamSidebandEvent::Closed)); // TODO: is it correct to send Closed here?
        }
    }

    pub fn on_disconnect(&mut self) {
        self.request_id = 1;
        self.connected_tx = None;
        // self.device_info = None;
        // self.max_protocol_mismatched_messages = 10;
        self.link_setup_done = false;
        self.packet_started_instant = None;
    }

    pub fn prune_timed_out_requests(&mut self) {
        let mut to_prune = vec![];
        let now = Instant::now();
        for (request_id, (_, prune_at)) in &self.response_map {
            if &now >= prune_at {
                to_prune.push(*request_id);
            }
        }
        for request_id in to_prune {
            if let Some((tx, _)) = self.response_map.remove(&request_id) {
                let _ = tx.send(Err(Error::Timeout));
            }
        }
    }

    pub fn on_connect(
        &mut self,
        on_error: OnError,
        connected_tx: Option<oneshot::Sender<Result<(), Error>>>,
        user_protocol_version: FullVersionOwned,
    ) {
        self.exit_on_error = on_error != OnError::KeepRetrying;
        self.request_id = 1;
        self.connected_tx = connected_tx; // sent after handshake is actually done
        self.user_protocol_version = Some(user_protocol_version);
    }
}

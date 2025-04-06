use crate::{DEFAULT_REQUEST_TIMEOUT, Error, SeqTy};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, oneshot};

pub type ResponseSender<E> = oneshot::Sender<Result<Vec<u8>, Error<E>>>;
pub type StreamUpdateSender<E> = mpsc::UnboundedSender<Result<Vec<u8>, Error<E>>>;

pub struct CommonState<E> {
    pub exit_on_error: bool,
    pub request_id: SeqTy,
    // user_protocol: ProtocolInfo,
    // conn_state: Arc<RwLock<ConnectionInfo>>,
    pub connected_tx: Option<oneshot::Sender<Result<(), Error<E>>>>,
    pub response_map: HashMap<SeqTy, (ResponseSender<E>, Instant)>,
    pub stream_handlers: HashMap<Vec<u16>, StreamUpdateSender<E>>,
    pub link_setup_done: bool,
    pub packet_started_instant: Option<Instant>,
    pub last_ping_instant: Option<Instant>,
    pub packet_accumulation_time: Duration,
}

impl<E> Default for CommonState<E> {
    fn default() -> Self {
        CommonState {
            exit_on_error: true,
            request_id: 1,
            connected_tx: None,
            response_map: Default::default(),
            stream_handlers: Default::default(),
            link_setup_done: false,
            packet_started_instant: None,
            last_ping_instant: None,
            packet_accumulation_time: Duration::from_millis(1),
        }
    }
}

impl<E> CommonState<E> {
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
        done_tx: Option<ResponseSender<E>>,
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
            let _ = tx.send(Err(Error::Disconnected));
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
}

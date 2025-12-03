use crate::{Error, OnError};
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use ww_version::FullVersionOwned;

pub struct CommonState {
    pub exit_on_error: bool,
    pub link_setup_done: bool,
    pub connected_tx: Option<oneshot::Sender<Result<(), Error>>>,
    pub packet_started_instant: Option<Instant>,
    pub last_rx_ping_instant: Option<Instant>,
    pub packet_accumulation_time: Duration,
    pub user_protocol_version: Option<FullVersionOwned>,
}

impl Default for CommonState {
    fn default() -> Self {
        CommonState {
            exit_on_error: true,
            link_setup_done: false,
            connected_tx: None,
            packet_started_instant: None,
            last_rx_ping_instant: None,
            packet_accumulation_time: Duration::from_millis(1),
            user_protocol_version: None,
        }
    }
}

impl CommonState {
    pub fn on_disconnect(&mut self) {
        self.link_setup_done = false;
        self.packet_started_instant = None;
        self.connected_tx = None;
    }

    pub fn on_connect(
        &mut self,
        on_error: OnError,
        connected_tx: Option<oneshot::Sender<Result<(), Error>>>,
        user_protocol_version: FullVersionOwned,
    ) {
        self.exit_on_error = on_error != OnError::KeepRetrying;
        self.connected_tx = connected_tx;
        self.user_protocol_version = Some(user_protocol_version);
    }
}

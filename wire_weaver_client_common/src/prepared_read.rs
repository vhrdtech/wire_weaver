use crate::command_sender::{DispatcherCommander, TransportCommander};
use crate::promise::Promise;
use crate::{Error, SeqTy};
use std::fmt::Debug;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};
use wire_weaver::prelude::DeserializeShrinkWrapOwned;
use ww_client_server::PathKindOwned;

/// Self-contained struct containing all necessary information needed to perform a read:
/// * TX ends towards transport and dispatcher event loops
/// * Resource path
/// * Property type as a generic `T` argument
///
/// When obtained, the user can choose how to actually execute the call:
/// * async: `read()`
/// * blocking: `blocking_read()`
/// * turn into a `Promise<T>` useful in immediate mode UI
#[must_use = "PrepareRead does nothing, unless read(), blocking_read() or read_promise() is used"]
pub struct PreparedRead<T> {
    pub(crate) transport_cmd_tx: TransportCommander,
    pub(crate) dispatcher_cmd_tx: DispatcherCommander,
    pub(crate) seq_rx: Arc<RwLock<mpsc::Receiver<SeqTy>>>,
    pub(crate) path_kind: Result<PathKindOwned, Error>,
    pub(crate) timeout: Option<Duration>,
    pub(crate) _phantom: PhantomData<T>,
}

impl<T: DeserializeShrinkWrapOwned + Debug> PreparedRead<T> {
    /// Use a provided timeout instead of the default one propagated from CommandSender
    pub fn with_timeout(self, timeout: Duration) -> Self {
        Self {
            transport_cmd_tx: self.transport_cmd_tx,
            dispatcher_cmd_tx: self.dispatcher_cmd_tx,
            seq_rx: self.seq_rx,
            path_kind: self.path_kind,
            timeout: Some(timeout),
            _phantom: PhantomData,
        }
    }

    /// Send read request, await a response (or timeout) and return it.
    pub async fn read(self) -> Result<T, Error> {
        // late error return, to have more ergonomic dev.fn_name().call()?; instead of dev.fn_name()?.call()?;
        let path_kind = self.path_kind?;

        // obtain next seq
        let seq = {
            let mut seq_rx = self.seq_rx.write().await;
            seq_rx.recv().await.ok_or(Error::RxDispatcherNotRunning)?
        };

        // notify rx dispatcher & send call to a remote device through transport layer
        let done_rx = self.dispatcher_cmd_tx.on_read_return(seq, self.timeout)?;
        self.transport_cmd_tx.send_read_request(seq, path_kind)?;

        // await return value from a remote device (routed through rx dispatcher)
        let rx_or_recv_err = done_rx.await.map_err(|_| Error::RxDispatcherNotRunning)?;
        let response = rx_or_recv_err?; // timeout is handled by rx dispatcher
        let reply: T = T::from_ww_bytes_owned(&response)?;
        Ok(reply)
    }

    /// Send read request, block the thread until the response is received (or timeout) and return it.
    pub fn blocking_read(self) -> Result<T, Error> {
        let path_kind = self.path_kind?;

        // obtain next seq
        let seq = {
            let mut seq_rx = self.seq_rx.blocking_write();
            seq_rx
                .blocking_recv()
                .ok_or(Error::RxDispatcherNotRunning)?
        };

        // notify rx dispatcher & send call to a remote device through transport layer
        let done_rx = self.dispatcher_cmd_tx.on_read_return(seq, self.timeout)?;
        self.transport_cmd_tx.send_read_request(seq, path_kind)?;

        // await return value from a remote device (routed through rx dispatcher)
        let rx_or_recv_err = done_rx
            .blocking_recv()
            .map_err(|_| Error::RxDispatcherNotRunning)?;
        let response = rx_or_recv_err?; // timeout is handled by rx dispatcher
        let reply: T = T::from_ww_bytes_owned(&response)?;
        Ok(reply)
    }

    /// Send a read request and return a Promise that can be used to await a result. Useful for immediate mode UI.
    #[must_use = "Promise does nothing, unless it is polled"]
    pub fn read_promise(self, marker: &'static str) -> Promise<T> {
        let path_kind = match self.path_kind {
            Ok(p) => p,
            Err(e) => return Promise::error(e, marker),
        };

        Promise::new_read(
            path_kind,
            self.seq_rx,
            self.timeout,
            self.transport_cmd_tx,
            self.dispatcher_cmd_tx,
            marker,
        )
    }
}

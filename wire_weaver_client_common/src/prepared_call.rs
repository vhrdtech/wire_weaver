use crate::command_sender::{DispatcherCommander, TransportCommander};
use crate::promise::Promise;
use crate::{Error, SeqTy};
use std::fmt::Debug;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{RwLock, mpsc};
use wire_weaver::prelude::DeserializeShrinkWrapOwned;
use ww_client_server::PathKindOwned;

/// Self-contained struct containing all necessary information needed to perform a call:
/// * TX ends towards transport and dispatcher event loops
/// * Serialized method arguments
/// * Resource path
/// * Return type as a generic `T` argument
///
/// When obtained, user can choose how to actually execute the call:
/// * async: `call()`
/// * blocking: `blocking_call()`
/// * call-ignoring-return value: `call_forget()`
/// * turn into a `Promise<T>` useful in immediate mode UI
#[must_use = "PrepareCall does nothing, unless call(), blocking_call(), call_forget() or call_promise() is used"]
pub struct PreparedCall<T> {
    pub(crate) transport_cmd_tx: TransportCommander,
    pub(crate) dispatcher_cmd_tx: DispatcherCommander,
    pub(crate) seq_rx: Arc<RwLock<mpsc::Receiver<SeqTy>>>,
    pub(crate) path_kind: Result<PathKindOwned, Error>,
    pub(crate) args: Result<Vec<u8>, Error>,
    pub(crate) timeout: Option<Duration>,
    pub(crate) _phantom: PhantomData<T>,
}

impl<T: DeserializeShrinkWrapOwned + Debug> PreparedCall<T> {
    /// Use provided timeout instead of default one propagated from CommandSender
    pub fn with_timeout(self, timeout: Duration) -> Self {
        Self {
            transport_cmd_tx: self.transport_cmd_tx,
            dispatcher_cmd_tx: self.dispatcher_cmd_tx,
            seq_rx: self.seq_rx,
            path_kind: self.path_kind,
            args: self.args,
            timeout: Some(timeout),
            _phantom: PhantomData,
        }
    }

    /// Send call request, await response (or timeout) and return it.
    pub async fn call(self) -> Result<T, Error> {
        // late error return, to have more ergonomic dev.fn_name().call()?; instead of dev.fn_name()?.call()?;
        let path_kind = self.path_kind?;
        let args = self.args?;

        // obtain next seq
        let seq = {
            let mut seq_rx = self.seq_rx.write().await;
            seq_rx.recv().await.ok_or(Error::RxDispatcherNotRunning)?
        };

        // notify rx dispatcher & send call to remote device through transport layer
        let done_rx = self.dispatcher_cmd_tx.on_call_return(seq, self.timeout)?;
        self.transport_cmd_tx
            .send_call_request(seq, path_kind, args)?;

        // await return value from remote device (routed through rx dispatcher)
        let rx_or_recv_err = done_rx.await.map_err(|_| Error::RxDispatcherNotRunning)?;
        let response = rx_or_recv_err?; // timeout is handled by rx dispatcher
        let reply: T = T::from_ww_bytes_owned(&response)?;
        Ok(reply)
    }

    /// Send call request, block the thread until response is received (or timeout) and return it.
    pub fn blocking_call(self) -> Result<T, Error> {
        let path_kind = self.path_kind?;
        let args = self.args?;

        // obtain next seq
        let seq = {
            let mut seq_rx = self.seq_rx.blocking_write();
            seq_rx
                .blocking_recv()
                .ok_or(Error::RxDispatcherNotRunning)?
        };

        // notify rx dispatcher & send call to remote device through transport layer
        let done_rx = self.dispatcher_cmd_tx.on_call_return(seq, self.timeout)?;
        self.transport_cmd_tx
            .send_call_request(seq, path_kind, args)?;

        // await return value from remote device (routed through rx dispatcher)
        let rx_or_recv_err = done_rx
            .blocking_recv()
            .map_err(|_| Error::RxDispatcherNotRunning)?;
        let response = rx_or_recv_err?; // timeout is handled by rx dispatcher
        let reply: T = T::from_ww_bytes_owned(&response)?;
        Ok(reply)
    }

    /// Send call request with seq = 0 and immediately return without response (remote end won't send it either).
    pub fn call_forget(self) -> Result<(), Error> {
        let path_kind = self.path_kind?;
        let args = self.args?;
        self.transport_cmd_tx
            .send_call_request(0, path_kind, args)?;
        Ok(())
    }

    /// Send call request and return a Promise that can be used to await a result. Useful for immediate mode UI.
    pub fn call_promise(self, marker: &'static str) -> Promise<T> {
        let path_kind = match self.path_kind {
            Ok(p) => p,
            Err(e) => return Promise::error(e, marker),
        };
        let args = match self.args {
            Ok(a) => a,
            Err(e) => return Promise::error(e, marker),
        };

        Promise::new_call(
            path_kind,
            args,
            self.seq_rx,
            self.timeout,
            self.transport_cmd_tx,
            self.dispatcher_cmd_tx,
            marker,
        )
    }
}

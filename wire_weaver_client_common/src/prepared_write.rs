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
/// * Optional write error type as a generic `E` argument
///
/// When obtained, the user can choose how to actually execute the call:
/// * async: `read()`
/// * blocking: `blocking_read()`
/// * turn into a `Promise<T>` useful in immediate mode UI
#[must_use = "PrepareRead does nothing, unless read(), blocking_read() or read_promise() is used"]
pub struct PreparedWrite<E> {
    pub(crate) postpone_err: Result<(), Error>,
    pub(crate) transport_cmd_tx: TransportCommander,
    pub(crate) dispatcher_cmd_tx: DispatcherCommander,
    pub(crate) seq_rx: Arc<RwLock<mpsc::Receiver<SeqTy>>>,
    pub(crate) path_kind: PathKindOwned,
    pub(crate) value: Vec<u8>,
    pub(crate) timeout: Option<Duration>,
    pub(crate) _phantom_err: PhantomData<E>,
}

impl<E: DeserializeShrinkWrapOwned + Debug> PreparedWrite<E> {
    /// Use provided timeout instead of default one propagated from CommandSender
    pub fn with_timeout(self, timeout: Duration) -> Self {
        Self {
            postpone_err: self.postpone_err,
            transport_cmd_tx: self.transport_cmd_tx,
            dispatcher_cmd_tx: self.dispatcher_cmd_tx,
            seq_rx: self.seq_rx,
            path_kind: self.path_kind,
            value: self.value,
            timeout: Some(timeout),
            _phantom_err: PhantomData,
        }
    }

    /// Send write request, await response (or timeout) and return it.
    pub async fn write(self) -> Result<(), Error> {
        // late error return, to have more ergonomic dev.fn_name().call()?; instead of dev.fn_name()?.call()?;
        self.postpone_err?;

        // obtain next seq
        let seq = {
            let mut seq_rx = self.seq_rx.write().await;
            seq_rx.recv().await.ok_or(Error::RxDispatcherNotRunning)?
        };

        // notify rx dispatcher & send call to a remote device through transport layer
        let done_rx = self.dispatcher_cmd_tx.on_write_return(seq, self.timeout)?;
        self.transport_cmd_tx
            .send_write_request(seq, self.path_kind, self.value)?;

        // await return value from a remote device (routed through rx dispatcher)
        let rx_or_recv_err = done_rx.await.map_err(|_| Error::RxDispatcherNotRunning)?;
        let _empty = rx_or_recv_err?; // timeout is handled by rx dispatcher
        Ok(())
    }

    /// Send write request, block the thread until response is received (or timeout) and return it.
    pub fn blocking_write(self) -> Result<(), Error> {
        self.postpone_err?;

        // obtain next seq
        let seq = {
            let mut seq_rx = self.seq_rx.blocking_write();
            seq_rx
                .blocking_recv()
                .ok_or(Error::RxDispatcherNotRunning)?
        };

        // notify rx dispatcher & send call to a remote device through transport layer
        let done_rx = self.dispatcher_cmd_tx.on_write_return(seq, self.timeout)?;
        self.transport_cmd_tx
            .send_write_request(seq, self.path_kind, self.value)?;

        // await return value from a remote device (routed through rx dispatcher)
        let rx_or_recv_err = done_rx
            .blocking_recv()
            .map_err(|_| Error::RxDispatcherNotRunning)?;
        let _empty = rx_or_recv_err?; // timeout is handled by rx dispatcher
        Ok(())
    }

    /// Send write request with seq = 0 and immediately return without response (remote end won't send it either).
    pub fn write_forget(self) -> Result<(), Error> {
        self.postpone_err?;
        self.transport_cmd_tx
            .send_write_request(0, self.path_kind, self.value)?;
        Ok(())
    }

    /// Send write request and return a Promise that can be used to await a result. Useful for immediate mode UI.
    #[must_use = "Promise does nothing, unless it is polled"]
    pub fn write_promise(self, marker: &'static str) -> Promise<E> {
        if let Err(e) = self.postpone_err {
            return Promise::error(e, marker);
        }

        Promise::new_write(
            self.path_kind,
            self.value,
            self.seq_rx,
            self.timeout,
            self.transport_cmd_tx,
            self.dispatcher_cmd_tx,
            marker,
        )
    }
}

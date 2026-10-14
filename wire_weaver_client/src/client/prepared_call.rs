use crate::event_loop::commander::TransportCommander;
use crate::{DecodeReply, Error, Promise};
use std::marker::PhantomData;
use std::time::Duration;
use wire_weaver::prelude::DeserializeShrinkWrapOwned;
use ww_client_server::PathKindOwned;

/// Self-contained struct containing all necessary information needed to perform a call:
/// * TX ends towards transport and dispatcher event loops
/// * Serialized method arguments
/// * Resource path
/// * Return type as a generic `T` argument
/// * Error type as a generic `E` argument: [Error] for methods without `Result`, [CallError](crate::CallError) for
///   methods returning `Result<T, E>`
///
/// When obtained, the user can choose how to actually execute the call:
/// * async: `call()`
/// * blocking: `blocking_call()`
/// * call-ignoring-return value: `call_forget()`
/// * turn into a `Promise<T, E>` useful in immediate mode UI
#[must_use = "PreparedCall does nothing, unless call(), blocking_call(), call_forget() or call_promise() is used"]
pub struct PreparedCall<T, E = Error> {
    pub(crate) postpone_err: Result<(), Error>,
    pub(crate) transport_cmd_tx: TransportCommander,
    pub(crate) path_kind: PathKindOwned,
    pub(crate) args: Vec<u8>,
    pub(crate) timeout_override: Option<Duration>,
    /// Names the method in [CallError::Unimplemented](crate::CallError::Unimplemented)
    pub(crate) resource: &'static str,
    pub(crate) _phantom: PhantomData<(T, E)>,
}

impl<T: DeserializeShrinkWrapOwned, E: DecodeReply<T>> PreparedCall<T, E> {
    /// Use a provided timeout instead of the default one propagated from CommandSender.
    ///
    /// Note that this timeout does not take into account possible command queue slot waiting.
    pub fn with_timeout(self, timeout: Duration) -> Self {
        Self {
            postpone_err: self.postpone_err,
            transport_cmd_tx: self.transport_cmd_tx,
            path_kind: self.path_kind,
            args: self.args,
            timeout_override: Some(timeout),
            resource: self.resource,
            _phantom: PhantomData,
        }
    }

    /// Send a call request, await a response (or timeout) and return it.
    ///
    /// Note that this method might have to await for command queue slot.
    /// This can happen if too many requests are sent or if client device is slow to respond.
    pub async fn call(self) -> Result<T, E> {
        let resource = self.resource;
        self.call_inner()
            .await
            .map_err(|e| E::from_client(e, resource))?
    }

    async fn call_inner(self) -> Result<Result<T, E>, Error> {
        // late error return, to have more ergonomic dev.fn_name().call()?; instead of dev.fn_name()?.call()?;
        self.postpone_err?;

        // send call to a remote device through transport layer
        let done_rx = self
            .transport_cmd_tx
            .send_call_request(self.path_kind, self.args, self.timeout_override)
            .await?;

        // await return value from a remote device (routed through rx dispatcher)
        let rx_or_recv_err = done_rx.await.map_err(|_| Error::RxDispatcherNotRunning)?;
        let response = rx_or_recv_err?; // timeout is handled by rx dispatcher
        Ok(E::decode_reply(&response))
    }

    /// Send a call request, block the thread until a response is received (or timeout) and return it.
    ///
    /// Note that this method might block waiting for command queue slot.
    /// This can happen if too many requests are sent or if client device is slow to respond.
    pub fn blocking_call(self) -> Result<T, E> {
        let resource = self.resource;
        self.blocking_call_inner()
            .map_err(|e| E::from_client(e, resource))?
    }

    fn blocking_call_inner(self) -> Result<Result<T, E>, Error> {
        self.postpone_err?;

        // send call to a remote device through transport layer
        let done_rx = self.transport_cmd_tx.send_call_request_blocking(
            self.path_kind,
            self.args,
            self.timeout_override,
        )?;

        // await return value from a remote device (routed through rx dispatcher)
        let rx_or_recv_err = done_rx
            .blocking_recv()
            .map_err(|_| Error::RxDispatcherNotRunning)?;
        let response = rx_or_recv_err?; // timeout is handled by rx dispatcher
        Ok(E::decode_reply(&response))
    }

    /// Send a call request with seq = 0 and immediately return without response (the remote end won't send it either).
    ///
    /// Note that this method might have to await for command queue slot.
    /// This can happen if too many requests are sent or if client device is slow to respond.
    pub async fn call_forget(self) -> Result<(), Error> {
        self.postpone_err?;
        self.transport_cmd_tx
            .send_call_request_forget(self.path_kind, self.args)
            .await?;
        Ok(())
    }

    /// Send a call request with seq = 0 and immediately return without response (the remote end won't send it either).
    ///
    /// Note that this method might block waiting for command queue slot.
    /// This can happen if too many requests are sent or if client device is slow to respond.
    pub fn blocking_call_forget(self) -> Result<(), Error> {
        self.postpone_err?;
        self.transport_cmd_tx
            .send_call_request_forget_blocking(self.path_kind, self.args)?;
        Ok(())
    }

    /// Return a [Promise] that sends a call request on its first poll and receives the result on subsequent polls.
    /// Useful for immediate mode UI. Must only be polled from synchronous code, not from async tasks,
    /// see [Promise] docs; use `call()` in async code.
    #[must_use = "Promise does nothing, unless it is polled"]
    pub fn call_promise(self, marker: &'static str) -> Promise<T, E> {
        if let Err(e) = self.postpone_err {
            return Promise::error(E::from_client(e, self.resource), marker);
        }

        Promise::new_call(
            self.path_kind,
            self.args,
            self.timeout_override,
            self.transport_cmd_tx,
            self.resource,
            marker,
        )
    }
}

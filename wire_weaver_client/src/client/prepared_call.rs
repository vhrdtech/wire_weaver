use crate::client::multi::CallPath;
use crate::error_list::{self, ErrorList};
use crate::event_loop::commander::TransportCommander;
use crate::{CallError, DecodeReply, Error, OutcomeError, Promise};
use std::fmt::Debug;
use std::marker::PhantomData;
use std::time::Duration;
use wire_weaver::prelude::DeserializeShrinkWrapOwned;
use wire_weaver::shrink_wrap::UNib32;
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
    /// `#[errors(..)]` of the method (API-32), checked on the device's error
    pub(crate) error_list: Option<ErrorList<E>>,
    pub(crate) _phantom: PhantomData<(T, E)>,
}

impl<T, D: DeserializeShrinkWrapOwned + Debug> PreparedCall<T, CallError<D>> {
    /// The method's `#[errors(..)]` list (API-32), used by generated client code: a device error outside it is
    /// logged as a warning (a panic with [set_strict_error_lists](crate::set_strict_error_lists)).
    pub fn with_error_list(mut self, list: ErrorList<CallError<D>>) -> Self {
        self.error_list = Some(list);
        self
    }
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
            error_list: self.error_list,
            _phantom: PhantomData,
        }
    }

    /// Send a call request, await a response (or timeout) and return it.
    ///
    /// Note that this method might have to await for command queue slot.
    /// This can happen if too many requests are sent or if client device is slow to respond.
    pub async fn call(self) -> Result<T, E> {
        let (resource, error_list) = (self.resource, self.error_list);
        let result = self
            .call_inner()
            .await
            .map_err(|e| E::from_client(e, resource))?;
        check_error_list(result, error_list.as_ref(), resource)
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
        let (resource, error_list) = (self.resource, self.error_list);
        let result = self
            .blocking_call_inner()
            .map_err(|e| E::from_client(e, resource))?;
        check_error_list(result, error_list.as_ref(), resource)
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

    /// Send the call request now and return its reply to await later (CLT-33).
    ///
    /// Requests sent one after another leave in that order, whatever order their replies are awaited in, so a host
    /// can keep several calls in flight and still have them reach the device in order (pipelined `ww_dfu::Stage`
    /// steps). The timeout runs from the send.
    pub async fn send(self) -> Result<PendingCall<T, E>, E> {
        let (resource, error_list) = (self.resource, self.error_list);
        let sent = async {
            self.postpone_err?;
            self.transport_cmd_tx
                .send_call_request(self.path_kind, self.args, self.timeout_override)
                .await
        };
        let done_rx = sent.await.map_err(|e| E::from_client(e, resource))?;
        Ok(PendingCall {
            done_rx,
            resource,
            error_list,
            _phantom: PhantomData,
        })
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
            self.error_list,
            marker,
        )
    }
}

impl<T: DeserializeShrinkWrapOwned> CallPath for PreparedCall<T, Error> {
    type Output = T;

    fn absolute_path(&self) -> Option<Vec<UNib32>> {
        if self.postpone_err.is_err() {
            return None;
        }
        match &self.path_kind {
            PathKindOwned::Absolute { path } => Some(path.clone()),
            _ => None,
        }
    }

    fn args_bytes(&self) -> Vec<u8> {
        self.args.clone()
    }

    fn commander(&self) -> TransportCommander {
        self.transport_cmd_tx.clone()
    }
}

/// Checks the device's error of a call against the method's `#[errors(..)]` list.
pub(crate) fn check_error_list<T, E: OutcomeError>(
    result: Result<T, E>,
    list: Option<&ErrorList<E>>,
    resource: &str,
) -> Result<T, E> {
    if let Err(e) = &result {
        error_list::check(e, list, resource);
    }
    result
}

/// A call whose request is sent, its reply not awaited yet: see [PreparedCall::send].
#[must_use = "the reply is lost unless awaited with reply()"]
pub struct PendingCall<T, E = Error> {
    done_rx: crate::event_loop::rx_dispatcher::ResponseReceiver,
    resource: &'static str,
    error_list: Option<ErrorList<E>>,
    _phantom: PhantomData<T>,
}

impl<T: DeserializeShrinkWrapOwned, E: DecodeReply<T>> PendingCall<T, E> {
    /// Await the reply (or the timeout given when the call was prepared).
    pub async fn reply(self) -> Result<T, E> {
        let resource = self.resource;
        let response = match self.done_rx.await {
            Ok(Ok(response)) => response,
            Ok(Err(e)) => return Err(E::from_client(e, resource)),
            Err(_) => return Err(E::from_client(Error::RxDispatcherNotRunning, resource)),
        };
        check_error_list(
            E::decode_reply(&response),
            self.error_list.as_ref(),
            resource,
        )
    }
}

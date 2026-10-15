use crate::client::multi::WritePath;
use crate::client::prepared_call::check_error_list;
use crate::error_list::ErrorList;
use crate::event_loop::commander::TransportCommander;
use crate::{CallError, Error, OutcomeError, Promise};
use std::fmt::Debug;
use std::marker::PhantomData;
use std::time::Duration;
use wire_weaver::shrink_wrap::UNib32;
use ww_client_server::PathKindOwned;

/// Self-contained struct containing all necessary information needed to perform a write:
/// * TX ends towards transport and dispatcher event loops
/// * Resource path
/// * Error type as a generic `E` argument: [Error] for properties without a user error type,
///   [CallError](crate::CallError) for properties with one (`property!(rw name: T, UserError)`)
///
/// When obtained, the user can choose how to actually execute the call:
/// * async: `write()`
/// * blocking: `blocking_write()`
/// * turn into a `Promise<(), E>` useful in immediate mode UI
#[must_use = "PreparedWrite does nothing, unless write(), blocking_write() or write_promise() is used"]
pub struct PreparedWrite<E = Error> {
    pub(crate) postpone_err: Result<(), Error>,
    pub(crate) transport_cmd_tx: TransportCommander,
    pub(crate) path_kind: PathKindOwned,
    pub(crate) value: Vec<u8>,
    pub(crate) timeout_override: Option<Duration>,
    /// Names the property in [CallError::Unimplemented](crate::CallError::Unimplemented)
    pub(crate) resource: &'static str,
    /// `#[errors(..)]` of the property (API-32), checked on the device's error
    pub(crate) error_list: Option<ErrorList<E>>,
    pub(crate) _phantom_err: PhantomData<E>,
}

impl<D: wire_weaver::prelude::DeserializeShrinkWrapOwned + Debug> PreparedWrite<CallError<D>> {
    /// The property's `#[errors(..)]` list (API-32), used by generated client code: a device error outside it is
    /// logged as a warning (a panic with [set_strict_error_lists](crate::set_strict_error_lists)).
    pub fn with_error_list(mut self, list: ErrorList<CallError<D>>) -> Self {
        self.error_list = Some(list);
        self
    }
}

impl<E: OutcomeError> PreparedWrite<E> {
    /// Use provided timeout instead of default one propagated from CommandSender
    pub fn with_timeout(self, timeout: Duration) -> Self {
        Self {
            postpone_err: self.postpone_err,
            transport_cmd_tx: self.transport_cmd_tx,
            path_kind: self.path_kind,
            value: self.value,
            timeout_override: Some(timeout),
            resource: self.resource,
            error_list: self.error_list,
            _phantom_err: PhantomData,
        }
    }

    /// Send write request, await response (or timeout) and return it.
    pub async fn write(self) -> Result<(), E> {
        let (resource, error_list) = (self.resource, self.error_list);
        let result = self
            .write_inner()
            .await
            .map_err(|e| E::from_client(e, resource));
        check_error_list(result, error_list.as_ref(), resource)
    }

    async fn write_inner(self) -> Result<(), Error> {
        // late error return, to have more ergonomic dev.fn_name().call()?; instead of dev.fn_name()?.call()?;
        self.postpone_err?;

        // send call to a remote device through transport layer
        let done_rx = self
            .transport_cmd_tx
            .send_write_request(self.path_kind, self.value, self.timeout_override)
            .await?;

        // await return value from a remote device (routed through rx dispatcher)
        let rx_or_recv_err = done_rx.await.map_err(|_| Error::RxDispatcherNotRunning)?;
        let _empty = rx_or_recv_err?; // timeout is handled by rx dispatcher
        Ok(())
    }

    /// Send write request, block the thread until response is received (or timeout) and return it.
    pub fn blocking_write(self) -> Result<(), E> {
        let (resource, error_list) = (self.resource, self.error_list);
        let result = self
            .blocking_write_inner()
            .map_err(|e| E::from_client(e, resource));
        check_error_list(result, error_list.as_ref(), resource)
    }

    fn blocking_write_inner(self) -> Result<(), Error> {
        self.postpone_err?;

        // send call to a remote device through transport layer
        let done_rx = self.transport_cmd_tx.send_write_request_blocking(
            self.path_kind,
            self.value,
            self.timeout_override,
        )?;

        // await return value from a remote device (routed through rx dispatcher)
        let rx_or_recv_err = done_rx
            .blocking_recv()
            .map_err(|_| Error::RxDispatcherNotRunning)?;
        let _empty = rx_or_recv_err?; // timeout is handled by rx dispatcher
        Ok(())
    }

    /// Send write request with seq = 0 and immediately return without response (remote end won't send it either).
    pub async fn write_forget(self) -> Result<(), Error> {
        self.postpone_err?;
        self.transport_cmd_tx
            .send_write_request_forget(self.path_kind, self.value)
            .await?;
        Ok(())
    }

    /// Send write request with seq = 0 and immediately return without response (remote end won't send it either).
    pub fn blocking_write_forget(self) -> Result<(), Error> {
        self.postpone_err?;
        self.transport_cmd_tx
            .send_write_request_forget_blocking(self.path_kind, self.value)?;
        Ok(())
    }

    /// Return a [Promise] that sends a write request on its first poll and receives the result on subsequent polls.
    /// Useful for immediate mode UI. Must only be polled from synchronous code, not from async tasks,
    /// see [Promise] docs; use `write()` in async code.
    #[must_use = "Promise does nothing, unless it is polled"]
    pub fn write_promise(self, marker: &'static str) -> Promise<(), E> {
        if let Err(e) = self.postpone_err {
            return Promise::error(E::from_client(e, self.resource), marker);
        }

        Promise::new_write(
            self.path_kind,
            self.value,
            self.timeout_override,
            self.transport_cmd_tx,
            self.resource,
            self.error_list,
            marker,
        )
    }
}

impl WritePath for PreparedWrite<Error> {
    fn absolute_path(&self) -> Option<Vec<UNib32>> {
        if self.postpone_err.is_err() {
            return None;
        }
        match &self.path_kind {
            PathKindOwned::Absolute { path } => Some(path.clone()),
            _ => None,
        }
    }

    fn value_bytes(&self) -> Vec<u8> {
        self.value.clone()
    }

    fn commander(&self) -> TransportCommander {
        self.transport_cmd_tx.clone()
    }
}

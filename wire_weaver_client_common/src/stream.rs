use crate::command_sender::TransportCommander;
use crate::{Error, StreamEvent, TypedStreamEvent};
use std::marker::PhantomData;
use std::ops::ControlFlow;
use tokio::sync::mpsc::UnboundedReceiver;
use wire_weaver::shrink_wrap::DeserializeShrinkWrapOwned;
use wire_weaver::shrink_wrap::Error as SWError;
use wire_weaver::shrink_wrap::raw_slice::RawSliceOwned;
use ww_client_server::{PathKindOwned, StreamSidebandCommand, StreamSidebandEvent};

/// Stream of typed values from host to device.
/// Also holds a sideband channel.
pub struct Stream<T> {
    pub(crate) transport_cmd_tx: TransportCommander,
    // pub(crate) dispatcher_cmd_tx: DispatcherCommander,
    // pub(crate) seq_rx: Arc<RwLock<mpsc::Receiver<SeqTy>>>,
    pub(crate) path_kind: PathKindOwned,
    pub(crate) rx: UnboundedReceiver<StreamEvent>,
    pub(crate) _phantom: PhantomData<T>,
}

#[derive(thiserror::Error, Debug)]
pub enum StreamError {
    #[error("Stream was unexpectedly closed")]
    Closed,
    #[error("Failed to deserialize stream value: {:?}", .0)]
    DeserializeError(SWError),
    #[error("Got unexpected stream event: {:?}", .0)]
    UnexpectedEvent(StreamEvent),
    #[error(transparent)]
    Other(#[from] Error),
}

impl<T: DeserializeShrinkWrapOwned> Stream<T> {
    /// Send Open command through sideband channel
    pub fn open(&self) -> Result<(), StreamError> {
        self.sideband(StreamSidebandCommand::Open)
    }

    /// Send Close command through sideband channel
    pub fn close(&self) -> Result<(), StreamError> {
        self.sideband(StreamSidebandCommand::Close)
    }

    /// Send command through sideband channel
    pub fn sideband(&self, sideband_cmd: StreamSidebandCommand) -> Result<(), StreamError> {
        self.transport_cmd_tx
            .send_stream_sideband(0, self.path_kind.clone(), sideband_cmd)?;
        Ok(())
    }

    /// Receive one data event and deserialize it.
    /// Returns an error if any kind of sideband event is received instead.
    ///
    /// See [Self::recv_blocking] for a blocking variant of this method.
    pub async fn recv(&mut self) -> Result<T, StreamError> {
        let ev = self.rx.recv().await.ok_or(StreamError::Closed)?;
        let StreamEvent::Data(bytes) = &ev else {
            return Err(StreamError::UnexpectedEvent(ev));
        };
        let data = T::from_ww_bytes_owned(bytes)?;
        Ok(data)
    }

    /// Receive one data event in a blocking manner and deserialize it.
    /// Returns an error if any kind of sideband event is received instead.
    ///
    /// See [Self::recv] for an asynchronous variant of this method.
    pub fn recv_blocking(&mut self) -> Result<T, StreamError> {
        let ev = self.rx.blocking_recv().ok_or(StreamError::Closed)?;
        let StreamEvent::Data(bytes) = &ev else {
            return Err(StreamError::UnexpectedEvent(ev));
        };
        let data = T::from_ww_bytes_owned(bytes)?;
        Ok(data)
    }

    /// Receive one event of any kind (data or sideband), deserialize if data is received.
    ///
    /// See [Self::recv_any_blocking] for a blocking variant of this method.
    pub async fn recv_any(&mut self) -> Result<TypedStreamEvent<T>, StreamError> {
        let ev = self.rx.recv().await.ok_or(StreamError::Closed)?;
        to_typed(ev)
    }

    /// Receive one event of any kind (data or sideband), deserialize if data is received.
    ///
    /// See [Self::recv_any] for an asynchronous variant of this method.
    pub fn recv_any_blocking(&mut self) -> Result<TypedStreamEvent<T>, StreamError> {
        let ev = self.rx.blocking_recv().ok_or(StreamError::Closed)?;
        to_typed(ev)
    }
}

impl Stream<RawSliceOwned> {
    /// Receive and accumulate bytes from this stream until Close sideband event is received.
    /// Returns an error if any other kind of sideband event is received instead.
    pub async fn recv_all_bytes(&mut self) -> Result<Vec<u8>, StreamError> {
        let mut bytes = vec![];
        while let Some(ev) = self.rx.recv().await {
            let f = recv_all_inner(ev, &mut bytes)?;
            if matches!(f, ControlFlow::Break(_)) {
                break;
            }
        }
        Ok(bytes)
    }

    /// Receive and accumulate bytes from this stream in a blocking manner until Close sideband event is received.
    /// Returns an error if any other kind of sideband event is received instead.
    pub fn recv_all_bytes_blocking(&mut self) -> Result<Vec<u8>, StreamError> {
        let mut bytes = vec![];
        while let Some(ev) = self.rx.blocking_recv() {
            let f = recv_all_inner(ev, &mut bytes)?;
            if matches!(f, ControlFlow::Break(_)) {
                break;
            }
        }
        Ok(bytes)
    }
}

impl From<SWError> for StreamError {
    fn from(e: SWError) -> Self {
        StreamError::DeserializeError(e)
    }
}

fn recv_all_inner(ev: StreamEvent, buf: &mut Vec<u8>) -> Result<ControlFlow<()>, StreamError> {
    match ev {
        StreamEvent::Data(b) => {
            buf.extend_from_slice(&b);
            Ok(ControlFlow::Continue(()))
        }
        StreamEvent::Connected => Ok(ControlFlow::Continue(())),
        StreamEvent::Sideband(StreamSidebandEvent::Closed) => Ok(ControlFlow::Break(())),
        e => Err(StreamError::UnexpectedEvent(e)),
    }
}

fn to_typed<T: DeserializeShrinkWrapOwned>(
    ev: StreamEvent,
) -> Result<TypedStreamEvent<T>, StreamError> {
    match ev {
        StreamEvent::Data(bytes) => {
            let data = T::from_ww_bytes_owned(&bytes)?;
            Ok(TypedStreamEvent::Data(data))
        }
        StreamEvent::Sideband(s) => Ok(TypedStreamEvent::Sideband(s)),
        StreamEvent::Connected => Ok(TypedStreamEvent::Connected),
        StreamEvent::Disconnected => Ok(TypedStreamEvent::Disconnected),
    }
}

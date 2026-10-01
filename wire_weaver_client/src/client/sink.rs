use crate::event_loop::commander::TransportCommander;
use crate::{StreamError, StreamEvent};
use std::marker::PhantomData;
use tokio::sync::mpsc::{UnboundedReceiver, error::TryRecvError};
use wire_weaver::shrink_wrap::SerializeShrinkWrap;
use wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned;
use ww_client_server::{PathKindOwned, StreamSideband};

/// Stream of typed values from host to device.
/// Also holds a sideband channel, in both directions.
pub struct Sink<T> {
    pub(crate) transport_cmd_tx: TransportCommander,
    pub(crate) path_kind: PathKindOwned,
    pub(crate) sideband_rx: UnboundedReceiver<StreamEvent>,
    pub(crate) _phantom: PhantomData<T>,
    pub(crate) scratch: [u8; 1024], // TODO: replace with Vec
}

impl<T> Sink<T> {
    /// Send Open command through the sideband channel
    pub async fn open(&self) -> Result<(), StreamError> {
        self.sideband(StreamSideband::Open).await
    }

    /// Send Open command through the sideband channel
    pub fn open_blocking(&self) -> Result<(), StreamError> {
        self.sideband_blocking(StreamSideband::Open)
    }

    /// Send Close command through the sideband channel
    pub async fn close(&self) -> Result<(), StreamError> {
        self.sideband(StreamSideband::Close).await
    }

    /// Send Close command through the sideband channel
    pub fn close_blocking(&self) -> Result<(), StreamError> {
        self.sideband_blocking(StreamSideband::Close)
    }

    /// Send command through the sideband channel
    pub async fn sideband(&self, sideband: StreamSideband) -> Result<(), StreamError> {
        self.transport_cmd_tx
            .send_stream_sideband(self.path_kind.clone(), sideband, None)
            .await?;
        Ok(())
    }

    /// Send command through the sideband channel
    pub fn sideband_blocking(&self, sideband: StreamSideband) -> Result<(), StreamError> {
        self.transport_cmd_tx.send_stream_sideband_blocking(
            self.path_kind.clone(),
            sideband,
            None,
        )?;
        Ok(())
    }

    /// Receive one sideband event from the device: a reply to [Self::sideband] or one sent by the device on its own.
    /// Skip [StreamEvent::Connected] events.
    /// Returns an error if a Disconnected event is received instead.
    ///
    /// See [Self::recv_sideband_blocking] for a blocking variant of this method.
    pub async fn recv_sideband(&mut self) -> Result<StreamSideband, StreamError> {
        loop {
            let ev = self.sideband_rx.recv().await.ok_or(StreamError::Closed)?;
            if let Some(sideband) = sideband_or_err(ev)? {
                return Ok(sideband);
            }
        }
    }

    /// Receive one sideband event from the device in a blocking manner.
    /// Skip [StreamEvent::Connected] events.
    /// Returns an error if a Disconnected event is received instead.
    ///
    /// See [Self::recv_sideband] for an asynchronous variant of this method.
    pub fn recv_sideband_blocking(&mut self) -> Result<StreamSideband, StreamError> {
        loop {
            let ev = self
                .sideband_rx
                .blocking_recv()
                .ok_or(StreamError::Closed)?;
            if let Some(sideband) = sideband_or_err(ev)? {
                return Ok(sideband);
            }
        }
    }

    /// Try to receive one sideband event from the device, `None` if there is none yet.
    /// Skip [StreamEvent::Connected] events.
    /// Returns an error if a Disconnected event is received instead.
    pub fn try_recv_sideband(&mut self) -> Result<Option<StreamSideband>, StreamError> {
        loop {
            match self.sideband_rx.try_recv() {
                Ok(ev) => {
                    if let Some(sideband) = sideband_or_err(ev)? {
                        return Ok(Some(sideband));
                    }
                }
                Err(TryRecvError::Empty) => return Ok(None),
                Err(TryRecvError::Disconnected) => return Err(StreamError::Closed),
            }
        }
    }

    /// Receive one event of any kind: sideband, Connected or Disconnected.
    ///
    /// See [Self::recv_any_blocking] for a blocking variant of this method.
    pub async fn recv_any(&mut self) -> Result<StreamEvent, StreamError> {
        self.sideband_rx.recv().await.ok_or(StreamError::Closed)
    }

    /// Receive one event of any kind: sideband, Connected or Disconnected.
    ///
    /// See [Self::recv_any] for an asynchronous variant of this method.
    pub fn recv_any_blocking(&mut self) -> Result<StreamEvent, StreamError> {
        self.sideband_rx.blocking_recv().ok_or(StreamError::Closed)
    }
}

fn sideband_or_err(ev: StreamEvent) -> Result<Option<StreamSideband>, StreamError> {
    match ev {
        StreamEvent::Sideband(sideband) => Ok(Some(sideband)),
        StreamEvent::Connected => Ok(None),
        ev => Err(StreamError::UnexpectedEvent(ev)),
    }
}

// Intentionally not applicable when byte slices are used (TailBytesOwned does not implement SerializeShrinkWrap)
// impl below for TailBytesOwned is provided instead
impl<T: SerializeShrinkWrap> Sink<T> {
    // TODO: remove &mut when scratch is no longer needed
    /// Serialize and send the provided value to a remote device sink
    pub async fn send(&mut self, value: T) -> Result<(), StreamError> {
        let value_bytes = value.to_ww_bytes(&mut self.scratch)?;
        self.transport_cmd_tx
            .send_write_request_forget(self.path_kind.clone(), value_bytes.to_vec())
            .await?;
        Ok(())
    }
}

impl Sink<TailBytesOwned> {
    pub async fn send_bytes(&mut self, bytes: &[u8]) -> Result<(), StreamError> {
        self.transport_cmd_tx
            .send_write_request_forget(self.path_kind.clone(), bytes.to_vec())
            .await?;
        Ok(())
    }

    pub fn send_bytes_blocking(&mut self, bytes: &[u8]) -> Result<(), StreamError> {
        self.transport_cmd_tx
            .send_write_request_forget_blocking(self.path_kind.clone(), bytes.to_vec())?;
        Ok(())
    }
}

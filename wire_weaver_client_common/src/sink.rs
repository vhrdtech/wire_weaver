use crate::command_sender::TransportCommander;
use crate::{StreamError, StreamEvent};
use std::marker::PhantomData;
use tokio::sync::mpsc::UnboundedReceiver;
use wire_weaver::shrink_wrap::SerializeShrinkWrap;
use wire_weaver::shrink_wrap::raw_slice::RawSliceOwned;
use ww_client_server::{PathKindOwned, StreamSidebandCommand};

/// Stream of typed values from device to host.
/// Also holds a sideband channel.
pub struct Sink<T> {
    pub(crate) transport_cmd_tx: TransportCommander,
    // pub(crate) dispatcher_cmd_tx: DispatcherCommander,
    // pub(crate) seq_rx: Arc<RwLock<mpsc::Receiver<SeqTy>>>,
    pub(crate) path_kind: PathKindOwned,
    pub(crate) _sideband_rx: UnboundedReceiver<StreamEvent>,
    pub(crate) _phantom: PhantomData<T>,
    pub(crate) scratch: [u8; 1024], // TODO: replace with Vec
}

impl<T> Sink<T> {
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
}

// Intentionally not applicable when byte slices are used (RawSliceOwned does not implement SerializeShrinkWrap)
// impl below for RawSliceOwned is provided instead
impl<T: SerializeShrinkWrap> Sink<T> {
    // TODO: remove &mut when scratch is no longer needed
    /// Serialize and send the provided value to a remote device sink
    pub fn send(&mut self, value: T) -> Result<(), StreamError> {
        let value_bytes = value.to_ww_bytes(&mut self.scratch)?;
        self.transport_cmd_tx.send_write_request(
            0,
            self.path_kind.clone(),
            value_bytes.to_vec(),
        )?;
        Ok(())
    }
}

impl Sink<RawSliceOwned> {
    pub fn send_bytes(&mut self, bytes: &[u8]) -> Result<(), StreamError> {
        self.transport_cmd_tx
            .send_write_request(0, self.path_kind.clone(), bytes.to_vec())?;
        Ok(())
    }
}

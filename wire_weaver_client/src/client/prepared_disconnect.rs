use tokio::sync::{mpsc, oneshot};
use wire_weaver::DisconnectReason;

use crate::{Error, internal::Command};

pub struct PreparedDisconnect {
    transport_cmd_tx: mpsc::Sender<Command>,
    reason: DisconnectReason,
    // pub(crate) timeout_override: Option<Duration>,
}

impl PreparedDisconnect {
    pub(crate) fn new(transport_cmd_tx: mpsc::Sender<Command>) -> Self {
        Self {
            transport_cmd_tx,
            reason: DisconnectReason::RequestByUser,
        }
    }

    pub fn reason(self, reason: DisconnectReason) -> Self {
        let mut s = self;
        s.reason = reason;
        s
    }

    /// Send disconnect command to a device and wait for it to go through, then stop the event loop and drop all remaining streams or requests.
    pub async fn asynch(self) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel::<()>();
        self.transport_cmd_tx
            .send(Command::DisconnectAndExit {
                disconnected_tx: Some(tx),
                reason: self.reason,
            })
            .await
            .map_err(|_| Error::EventLoopNotRunning)?;
        rx.await.map_err(|_| Error::EventLoopNotRunning)?;
        Ok(())
    }

    /// Send disconnect command to a device and wait for it to go through, then stop the event loop and drop all remaining streams or requests.
    pub fn blocking(self) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel::<()>();
        self.transport_cmd_tx
            .blocking_send(Command::DisconnectAndExit {
                disconnected_tx: Some(tx),
                reason: self.reason,
            })
            .map_err(|_| Error::EventLoopNotRunning)?;
        rx.blocking_recv().map_err(|_| Error::EventLoopNotRunning)?;
        Ok(())
    }

    /// Send disconnect command to a device, stop the event loop and drop all remaining streams or requests.
    pub(crate) fn forget(self) -> Result<(), Error> {
        self.transport_cmd_tx
            .try_send(Command::DisconnectAndExit {
                disconnected_tx: None,
                reason: self.reason,
            })
            .map_err(|_| Error::EventLoopNotRunning)?;
        Ok(())
    }

    /// Disconnect from a connected device. All streams will be kept and event loop will be left running ready for re-connect.
    pub async fn keep_streams(&self) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel::<()>();
        self.transport_cmd_tx
            .send(Command::DisconnectKeepStreams {
                disconnected_tx: Some(tx),
                reason: self.reason,
            })
            .await
            .map_err(|_| Error::EventLoopNotRunning)?;
        rx.await.map_err(|_| Error::EventLoopNotRunning)?;
        Ok(())
    }

    /// Disconnect from a connected device. All streams will be kept and event loop will be left running ready for re-connect.
    pub fn keep_streams_blocking(&self) -> Result<(), Error> {
        let (tx, rx) = oneshot::channel::<()>();
        self.transport_cmd_tx
            .blocking_send(Command::DisconnectKeepStreams {
                disconnected_tx: Some(tx),
                reason: self.reason,
            })
            .map_err(|_| Error::EventLoopNotRunning)?;
        rx.blocking_recv().map_err(|_| Error::EventLoopNotRunning)?;
        Ok(())
    }

    pub fn promise(self) -> () {
        todo!()
    }
}

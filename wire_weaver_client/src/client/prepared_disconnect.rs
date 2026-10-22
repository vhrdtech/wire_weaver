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

    /// Send disconnect command to a device, stop the event loop and drop all remaining streams or requests.
    /// Returns once the device is released and can be connected to again.
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

    /// Send disconnect command to a device, stop the event loop and drop all remaining streams or requests.
    /// Returns once the device is released and can be connected to again.
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

    /// Send disconnect command to a device, stop the event loop and drop all remaining streams or requests, from a
    /// `Drop` (no `.await`, maybe on a runtime thread). Waits until the event loop says the transport is closed, so
    /// a program that exits right after has said goodbye to the device, but no longer than `max_wait`. Returns at
    /// once when the event loop is gone already.
    pub(crate) fn on_drop(self, max_wait: std::time::Duration) {
        let (tx, mut rx) = oneshot::channel::<()>();
        if self
            .transport_cmd_tx
            .try_send(Command::DisconnectAndExit {
                disconnected_tx: Some(tx),
                reason: self.reason,
            })
            .is_err()
        {
            return;
        }
        let deadline = std::time::Instant::now() + max_wait;
        while std::time::Instant::now() < deadline {
            match rx.try_recv() {
                Err(oneshot::error::TryRecvError::Empty) => {
                    std::thread::sleep(std::time::Duration::from_millis(1))
                }
                // closed (said so, or the event loop is gone)
                _ => return,
            }
        }
    }

    /// Disconnect from a connected device. All streams will be kept and event loop will be left running ready for re-connect.
    /// Returns once the device is released and can be connected to again.
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
    /// Returns once the device is released and can be connected to again.
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

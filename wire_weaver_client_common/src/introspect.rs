use crate::command_sender::{DispatcherCommander, SeqRwLock, TransportCommander};
use crate::promise::Promise;
use crate::Error;
use wire_weaver::shrink_wrap::DeserializeShrinkWrapOwned;
use ww_self::ApiBundleOwned;

pub struct Introspect {
    transport_cmd_tx: TransportCommander,
    dispatcher_cmd_tx: DispatcherCommander,
    seq_rx: SeqRwLock,
}

impl Introspect {
    pub(crate) fn new(
        transport_cmd_tx: TransportCommander,
        dispatcher_cmd_tx: DispatcherCommander,
        seq_rx: SeqRwLock,
    ) -> Self {
        Introspect {
            transport_cmd_tx,
            dispatcher_cmd_tx,
            seq_rx,
        }
    }

    /// Request introspect data from a remote device.
    pub async fn download(self) -> Result<ApiBundleOwned, Error> {
        // obtain next seq
        let seq = {
            let mut seq_rx = self.seq_rx.write().await;
            seq_rx.recv().await.ok_or(Error::RxDispatcherNotRunning)?
        };
        // send introspect request
        self.transport_cmd_tx.send_introspect(seq)?;
        // notify rx dispatcher
        let mut rx = self.dispatcher_cmd_tx.on_introspect_chunk()?;
        // receive chunks
        let mut ww_self_bytes = vec![];
        while let Some(chunk) = rx.recv().await {
            if chunk.is_empty() {
                break;
            }
            ww_self_bytes.extend_from_slice(&chunk);
        }
        let api_bundle = ApiBundleOwned::from_ww_bytes_owned(&ww_self_bytes)?;
        Ok(api_bundle)
    }

    /// Receive all the introspect bytes chunks.
    pub fn download_blocking(self) -> Result<ApiBundleOwned, Error> {
        // obtain next seq
        let seq = {
            let mut seq_rx = self.seq_rx.blocking_write();
            seq_rx
                .blocking_recv()
                .ok_or(Error::RxDispatcherNotRunning)?
        };
        // send introspect request
        self.transport_cmd_tx.send_introspect(seq)?;
        // notify rx dispatcher
        let mut rx = self.dispatcher_cmd_tx.on_introspect_chunk()?;
        // receive chunks
        let mut ww_self_bytes = vec![];
        while let Some(chunk) = rx.blocking_recv() {
            if chunk.is_empty() {
                break;
            }
            ww_self_bytes.extend_from_slice(&chunk);
        }
        let api_bundle = ApiBundleOwned::from_ww_bytes_owned(&ww_self_bytes)?;
        Ok(api_bundle)
    }

    #[must_use = "Promise does nothing, unless it is polled"]
    pub fn download_promise(self) -> Promise<ApiBundleOwned> {
        Promise::new_introspect(
            self.seq_rx,
            self.transport_cmd_tx,
            self.dispatcher_cmd_tx,
            "introspect",
        )
    }
}

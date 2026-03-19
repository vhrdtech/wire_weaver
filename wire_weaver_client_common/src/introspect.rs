use crate::command_sender::TransportCommander;
use crate::promise::Promise;
use crate::{Error, Stream};
use wire_weaver::shrink_wrap::DeserializeShrinkWrapOwned;
use ww_client_server::PathKindOwned;
use ww_self::ApiBundleOwned;

pub struct Introspect {
    transport_cmd_tx: TransportCommander,
}

impl Introspect {
    pub(crate) fn new(transport_cmd_tx: TransportCommander) -> Self {
        Introspect { transport_cmd_tx }
    }

    /// Request introspect data from a remote device.
    pub async fn download(self) -> Result<ApiBundleOwned, Error> {
        // TODO: set introspect download data timeout
        let rx = self.transport_cmd_tx.send_introspect(None)?;
        let mut stream = Stream {
            transport_cmd_tx: self.transport_cmd_tx,
            path_kind: PathKindOwned::Absolute { path: vec![] },
            rx,
            _phantom: Default::default(),
        };
        let ww_self_bytes = stream
            .recv_all_bytes()
            .await
            .map_err(|e| Error::Other(e.to_string()))?;
        let api_bundle = ApiBundleOwned::from_ww_bytes_owned(&ww_self_bytes)?;
        Ok(api_bundle)
    }

    /// Receive all the introspect bytes chunks.
    pub fn download_blocking(self) -> Result<ApiBundleOwned, Error> {
        let rx = self.transport_cmd_tx.send_introspect(None)?;
        let mut stream = Stream {
            transport_cmd_tx: self.transport_cmd_tx,
            path_kind: PathKindOwned::Absolute { path: vec![] },
            rx,
            _phantom: Default::default(),
        };
        let ww_self_bytes = stream
            .recv_all_bytes_blocking()
            .map_err(|e| Error::Other(e.to_string()))?;
        let api_bundle = ApiBundleOwned::from_ww_bytes_owned(&ww_self_bytes)?;
        Ok(api_bundle)
    }

    #[must_use = "Promise does nothing, unless it is polled"]
    pub fn download_promise(self) -> Promise<ApiBundleOwned> {
        Promise::new_introspect(self.transport_cmd_tx, "introspect")
    }
}

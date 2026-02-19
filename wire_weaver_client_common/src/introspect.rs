use tokio::sync::mpsc;

pub struct Introspect {
    pub(crate) ww_self_chunk_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    ww_self_bytes: Vec<u8>,
}

#[derive(thiserror::Error, Debug)]
pub enum IntrospectError {
    #[error("Stream was unexpectedly closed")]
    AlreadyReceived,
}

impl Introspect {
    pub(crate) fn new(ww_self_chunk_rx: mpsc::UnboundedReceiver<Vec<u8>>) -> Self {
        Introspect {
            ww_self_chunk_rx,
            ww_self_bytes: vec![],
        }
    }

    /// Receive all the introspect bytes chunks.
    pub async fn recv(&mut self) -> Result<(), IntrospectError> {
        if !self.ww_self_bytes.is_empty() {
            return Err(IntrospectError::AlreadyReceived);
        }
        while let Some(chunk) = self.ww_self_chunk_rx.recv().await {
            if chunk.is_empty() {
                break;
            }
            self.ww_self_bytes.extend_from_slice(&chunk);
        }
        Ok(())
    }

    /// Receive all the introspect bytes chunks.
    pub fn recv_blocking(&mut self) -> Result<(), IntrospectError> {
        if !self.ww_self_bytes.is_empty() {
            return Err(IntrospectError::AlreadyReceived);
        }
        while let Some(chunk) = self.ww_self_chunk_rx.blocking_recv() {
            if chunk.is_empty() {
                break;
            }
            self.ww_self_bytes.extend_from_slice(&chunk);
        }
        Ok(())
    }

    /// Returns introspect bytes as received (serialized ww_self::ApiBundle)
    pub fn ww_self_bytes(&self) -> &[u8] {
        &self.ww_self_bytes
    }
}

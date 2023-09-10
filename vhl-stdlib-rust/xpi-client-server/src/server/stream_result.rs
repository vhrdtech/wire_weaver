use std::marker::PhantomData;
use serde::Serialize;
use tracing::{error, warn};
use xpi::client_server_owned::{AddressableEvent, Event, Nrl, Protocol, RequestId};
use futures::channel::mpsc::Sender;
use futures_util::SinkExt;
use thiserror::Error;

pub struct StreamResultContext<T> {
    pub source: Protocol,
    nrl: Nrl,
    seq: RequestId,
    events_tx: Sender<AddressableEvent>,

    _phantom: PhantomData<T>,
}

#[derive(Debug, Error)]
pub enum PublishError {
    #[error(transparent)]
    RmpEncode(#[from] rmp_serde::encode::Error),
    #[error("Mpsc send failed, client probably disconnected")]
    MpscError,
}

impl<T: Serialize> StreamResultContext<T> {
    pub fn new(source: Protocol, nrl: Nrl, seq: RequestId, events_tx: Sender<AddressableEvent>) -> Self {
        StreamResultContext {
            source,
            nrl,
            seq,
            events_tx,
            _phantom: PhantomData {},
        }
    }

    pub async fn publish_one(&mut self, item: T) -> Result<(), PublishError> {
        self.publish_many(&[item]).await
    }

    pub async fn publish_many(
        &mut self,
        items: &[T],
    ) -> Result<(), PublishError> {
        if items.is_empty() {
            return Ok(());
        }
        let mut data = Vec::new();
        serde::Serialize::serialize(items, &mut rmp_serde::Serializer::new(&mut data))?;
        let ev = Event::stream_update(self.nrl.clone(), data, self.seq);
        self.events_tx
            .send(AddressableEvent {
                protocol: self.source,
                is_inbound: false,
                event: ev,
                response_tx: self.events_tx.clone(), // TODO: weird
            })
            .await
            .map_err(|_| {
                PublishError::MpscError
            })?;
        Ok(())
    }

    pub async fn finish_stream(&mut self) -> Result<(), PublishError> {
        let ev = Event::stream_closed(self.nrl.clone(), self.seq);
        self.events_tx
            .send(AddressableEvent {
                protocol: self.source,
                is_inbound: false,
                event: ev,
                response_tx: self.events_tx.clone(),
            })
            .await
            .map_err(|_| {
                PublishError::MpscError
            })?;
        Ok(())
    }
}

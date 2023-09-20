use crate::util::IteratorAdapter;
use futures::channel::mpsc::Sender;
use futures_util::SinkExt;
use serde::Serialize;
use std::marker::PhantomData;
use thiserror::Error;
use tracing::error;
use xpi::client_server_owned::{AddressableEvent, Event, Nrl, Protocol, RequestId};

pub struct StreamResultContext<T> {
    source: Protocol,
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

impl<'a, T: Serialize + 'a> StreamResultContext<T> {
    pub fn new(request: &AddressableEvent) -> Self {
        StreamResultContext {
            source: request.protocol,
            nrl: request.event.nrl.clone(),
            seq: request.event.seq,
            events_tx: request.response_tx.clone(),
            _phantom: PhantomData {},
        }
    }

    pub async fn publish_one(&mut self, item: &'a T) -> Result<(), PublishError> {
        self.publish_many([item].into_iter()).await
    }

    pub async fn publish_many(
        &mut self,
        items: impl Iterator<Item = &'a T>,
    ) -> Result<(), PublishError> {
        // if items.is_empty() {
        //     return Ok(());
        // }
        let mut data = Vec::new();
        let iterator_adapter = IteratorAdapter::new(items);
        serde::Serialize::serialize(
            &iterator_adapter,
            &mut rmp_serde::Serializer::new(&mut data),
        )?;
        self.send_stream_update(data).await
    }

    pub async fn publish_many_owned(
        &mut self,
        items: impl Iterator<Item = T>,
    ) -> Result<(), PublishError> {
        let mut data = Vec::new();
        let iterator_adapter = IteratorAdapter::new(items);
        serde::Serialize::serialize(
            &iterator_adapter,
            &mut rmp_serde::Serializer::new(&mut data),
        )?;
        self.send_stream_update(data).await
    }

    async fn send_stream_update(&mut self, data: Vec<u8>) -> Result<(), PublishError> {
        let ev = Event::stream_update(self.nrl.clone(), data, self.seq);
        self.events_tx
            .send(AddressableEvent {
                protocol: self.source,
                is_inbound: false,
                event: ev,
                response_tx: self.events_tx.clone(), // TODO: weird
            })
            .await
            .map_err(|_| PublishError::MpscError)
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
            .map_err(|_| PublishError::MpscError)?;
        Ok(())
    }

    pub fn client_address(&self) -> Protocol {
        self.source
    }

    pub fn nrl(&self) -> Nrl {
        self.nrl.clone()
    }

    pub fn seq(&self) -> RequestId {
        self.seq
    }
}

use crate::Error;
use crate::client::batch::{BatchItem, Unpacker};
use crate::event_loop::commander::TransportCommander;
use crate::event_loop::event_queue::TryRecvError;
use crate::event_loop::rx_dispatcher::StreamUpdateReceiver;
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::ops::ControlFlow;
use std::time::{Duration, Instant};
use wire_weaver::shrink_wrap::DeserializeShrinkWrapOwned;
use wire_weaver::shrink_wrap::Error as SWError;
use wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned;
use ww_client_server::{PathKindOwned, StreamSideband};

/// Stream of typed values from device to host.
/// Also holds a sideband channel.
pub struct Stream<T> {
    pub(crate) transport_cmd_tx: TransportCommander,
    pub(crate) path_kind: PathKindOwned,
    pub(crate) rx: StreamUpdateReceiver,
    /// Batched streams (API-41): how an event becomes items
    pub(crate) unpacker: Unpacker<T>,
    /// Items of the last batch not taken yet
    pub(crate) pending: VecDeque<BatchItem<T>>,
    pub(crate) _phantom: PhantomData<T>,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum RawStreamEvent {
    /// Data channel from remote device
    Data(Vec<u8>),
    /// Sideband channel from remote device
    Sideband(StreamSideband),
    /// Locally generated event, sent when connection to remote device is up
    Connected,
    /// Locally generated event, sent when connection to remote device is down
    Disconnected,
    /// Locally generated event: the stream's queue on the host was full (the program didn't read for a while),
    /// `events` data events of `bytes` bytes before this one were dropped. See
    /// [ClientConfig::stream_queue_bytes](crate::ClientConfig::stream_queue_bytes).
    Dropped { events: u64, bytes: u64 },
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub enum TypedStreamEvent<T> {
    /// Data channel from remote device
    Data(T),
    /// Sideband channel from remote device
    Sideband(StreamSideband),
    /// Locally generated event, sent when connection to remote device is up
    Connected,
    /// Locally generated event, sent when connection to remote device is down
    Disconnected,
    /// Locally generated event: `events` data events of `bytes` bytes before this one were dropped, the stream's
    /// queue on the host was full.
    Dropped { events: u64, bytes: u64 },
}

#[derive(thiserror::Error, Debug)]
pub enum StreamError {
    #[error("Stream was unexpectedly closed")]
    Closed,
    #[error("Failed to deserialize stream value: {:?}", .0)]
    DeserializeError(SWError),
    #[error("Got unexpected stream event: {:?}", .0)]
    UnexpectedEvent(RawStreamEvent),
    /// A credit sink item never fits: credit counts bytes and items are never split, the device has to be able to
    /// buffer the largest one (`SinkCredit::sized_for` on the device checks it when the bound is known).
    #[error(
        "credit sink item of {len} bytes is bigger than the device's receive buffer of {buffer} bytes, it can never be sent"
    )]
    ItemTooBig { len: usize, buffer: u32 },
    #[error(transparent)]
    Other(#[from] Error),
}

impl<T: DeserializeShrinkWrapOwned> Stream<T> {
    /// Data events dropped since the stream was opened because its queue on the host was full (the program didn't
    /// read for a while), see [ClientConfig::stream_queue_bytes](crate::ClientConfig::stream_queue_bytes).
    pub fn dropped(&self) -> u64 {
        self.rx.dropped()
    }

    /// Send Close command through sideband channel
    pub async fn close(&self) -> Result<(), StreamError> {
        self.sideband(StreamSideband::Close).await
    }

    /// Send Close command through sideband channel
    pub fn close_blocking(&self) -> Result<(), StreamError> {
        self.sideband_blocking(StreamSideband::Close)
    }

    /// Send command through sideband channel
    pub async fn sideband(&self, sideband: StreamSideband) -> Result<(), StreamError> {
        // TODO: add synchronous mode to stream sideband (wait for response)?
        self.transport_cmd_tx
            .send_stream_sideband(self.path_kind.clone(), sideband, None)
            .await?;
        Ok(())
    }

    /// Send command through sideband channel
    pub fn sideband_blocking(&self, sideband: StreamSideband) -> Result<(), StreamError> {
        // TODO: add synchronous mode to stream sideband (wait for response)?
        self.transport_cmd_tx.send_stream_sideband_blocking(
            self.path_kind.clone(),
            sideband,
            None,
        )?;
        Ok(())
    }

    /// Receive one data event and deserialize it.
    /// Skip [RawStreamEvent::Connected] events.
    /// Returns an error if any kind of sideband or Disconnected event is received instead.
    ///
    /// See [Self::recv_blocking] for a blocking variant of this method.
    pub async fn recv(&mut self) -> Result<T, StreamError> {
        loop {
            if let Some(item) = self.pending.pop_front() {
                return Ok(item.value);
            }
            let bytes = self.recv_data().await?;
            if let Some(value) = self.decode(&bytes)? {
                return Ok(value);
            }
        }
    }

    /// The rest of the next batch of a batched stream (API-41), or the next one whole: its items, each with its
    /// device time (unwrapped, as in [StreamEvent::Item](crate::StreamEvent::Item)). An item of a stream that isn't
    /// batched is a batch of one, without a time.
    pub async fn recv_batch(&mut self) -> Result<Vec<BatchItem<T>>, StreamError> {
        loop {
            if !self.pending.is_empty() {
                return Ok(self.pending.drain(..).collect());
            }
            let bytes = self.recv_data().await?;
            self.decode_all(&bytes)?;
        }
    }

    /// Blocking variant of [Self::recv_batch].
    pub fn recv_batch_blocking(&mut self) -> Result<Vec<BatchItem<T>>, StreamError> {
        loop {
            if !self.pending.is_empty() {
                return Ok(self.pending.drain(..).collect());
            }
            let bytes = self.recv_data_blocking()?;
            self.decode_all(&bytes)?;
        }
    }

    /// Bytes of the next data event, skipping [RawStreamEvent::Connected] and [RawStreamEvent::Dropped].
    async fn recv_data(&mut self) -> Result<Vec<u8>, StreamError> {
        loop {
            let ev = self.rx.recv().await.ok_or(StreamError::Closed)?;
            if self.skip(&ev) {
                continue;
            }
            let RawStreamEvent::Data(bytes) = ev else {
                return Err(StreamError::UnexpectedEvent(ev));
            };
            return Ok(bytes);
        }
    }

    /// Blocking variant of [Self::recv_data].
    fn recv_data_blocking(&mut self) -> Result<Vec<u8>, StreamError> {
        loop {
            let ev = self.rx.blocking_recv().ok_or(StreamError::Closed)?;
            if self.skip(&ev) {
                continue;
            }
            let RawStreamEvent::Data(bytes) = ev else {
                return Err(StreamError::UnexpectedEvent(ev));
            };
            return Ok(bytes);
        }
    }

    /// [RawStreamEvent::Connected] and [RawStreamEvent::Dropped], which `recv` and friends skip; a new connection
    /// restarts the device time (the device may have rebooted).
    fn skip(&mut self, ev: &RawStreamEvent) -> bool {
        match ev {
            RawStreamEvent::Connected => {
                self.unpacker.time.restart();
                true
            }
            RawStreamEvent::Dropped { .. } => true,
            _ => false,
        }
    }

    /// The value of a data event; for a batched stream its first item, the others go to `pending` (None: an empty
    /// batch).
    fn decode(&mut self, bytes: &[u8]) -> Result<Option<T>, StreamError> {
        if !self.unpacker.is_batched() {
            return Ok(Some(T::from_ww_bytes_owned(bytes)?));
        }
        self.decode_all(bytes)?;
        Ok(self.pending.pop_front().map(|item| item.value))
    }

    /// Every item of a data event into `pending`.
    fn decode_all(&mut self, bytes: &[u8]) -> Result<(), StreamError> {
        if self.unpacker.is_batched() {
            let items = self.unpacker.unpack(bytes)?;
            self.pending.extend(items);
        } else {
            let value = T::from_ww_bytes_owned(bytes)?;
            self.pending.push_back(BatchItem { t: None, value });
        }
        Ok(())
    }

    /// Receive one data event in a blocking manner and deserialize it.
    /// Skip [RawStreamEvent::Connected] events.
    /// Returns an error if any kind of sideband or Disconnected event is received instead.
    ///
    /// See [Self::recv] for an asynchronous variant of this method.
    pub fn recv_blocking(&mut self) -> Result<T, StreamError> {
        loop {
            if let Some(item) = self.pending.pop_front() {
                return Ok(item.value);
            }
            let bytes = self.recv_data_blocking()?;
            if let Some(value) = self.decode(&bytes)? {
                return Ok(value);
            }
        }
    }

    /// Try to receive one data event and deserialize it.
    /// Skip [RawStreamEvent::Connected] events.
    /// Returns an error if any kind of sideband or Disconnected event is received instead.
    ///
    /// See [Self::recv] for an asynchronous variant of this method.
    pub fn try_recv(&mut self) -> Result<Option<T>, StreamError> {
        loop {
            if let Some(item) = self.pending.pop_front() {
                return Ok(Some(item.value));
            }
            match self.rx.try_recv() {
                Ok(ev) => {
                    if self.skip(&ev) {
                        continue;
                    }
                    let RawStreamEvent::Data(bytes) = ev else {
                        return Err(StreamError::UnexpectedEvent(ev));
                    };
                    if let Some(value) = self.decode(&bytes)? {
                        return Ok(Some(value));
                    }
                }
                Err(TryRecvError::Empty) => return Ok(None),
                Err(TryRecvError::Disconnected) => return Err(StreamError::Closed),
            }
        }
    }

    /// Receive one event of any kind (data or sideband), deserialize if data is received.
    ///
    /// See [Self::recv_any_blocking] for a blocking variant of this method.
    pub async fn recv_any(&mut self) -> Result<TypedStreamEvent<T>, StreamError> {
        loop {
            if let Some(item) = self.pending.pop_front() {
                return Ok(TypedStreamEvent::Data(item.value));
            }
            let ev = self.rx.recv().await.ok_or(StreamError::Closed)?;
            if let Some(ev) = self.type_event(ev)? {
                return Ok(ev);
            }
        }
    }

    /// Receive one event of any kind (data or sideband), deserialize if data is received.
    ///
    /// See [Self::recv_any] for an asynchronous variant of this method.
    pub fn recv_any_blocking(&mut self) -> Result<TypedStreamEvent<T>, StreamError> {
        loop {
            if let Some(item) = self.pending.pop_front() {
                return Ok(TypedStreamEvent::Data(item.value));
            }
            let ev = self.rx.blocking_recv().ok_or(StreamError::Closed)?;
            if let Some(ev) = self.type_event(ev)? {
                return Ok(ev);
            }
        }
    }

    /// A raw event typed; a batched stream's data event gives its first item (None: an empty batch).
    fn type_event(
        &mut self,
        ev: RawStreamEvent,
    ) -> Result<Option<TypedStreamEvent<T>>, StreamError> {
        Ok(Some(match ev {
            RawStreamEvent::Data(bytes) => match self.decode(&bytes)? {
                Some(value) => TypedStreamEvent::Data(value),
                None => return Ok(None),
            },
            RawStreamEvent::Sideband(s) => TypedStreamEvent::Sideband(s),
            RawStreamEvent::Connected => {
                self.unpacker.time.restart();
                TypedStreamEvent::Connected
            }
            RawStreamEvent::Disconnected => TypedStreamEvent::Disconnected,
            RawStreamEvent::Dropped { events, bytes } => {
                TypedStreamEvent::Dropped { events, bytes }
            }
        }))
    }
}

impl Stream<TailBytesOwned> {
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

    /// Same as [Self::recv_all_bytes], but fails with a timeout error if no event is received within `idle_timeout`.
    /// The timeout is restarted after each received event, so long transfers are not cut short.
    pub async fn recv_all_bytes_timeout(
        &mut self,
        idle_timeout: Duration,
    ) -> Result<Vec<u8>, StreamError> {
        let mut bytes = vec![];
        loop {
            let ev = tokio::time::timeout(idle_timeout, self.rx.recv())
                .await
                .map_err(|_| Error::Timeout)?
                .ok_or(StreamError::Closed)?;
            if recv_all_inner(ev, &mut bytes)?.is_break() {
                return Ok(bytes);
            }
        }
    }

    /// Same as [Self::recv_all_bytes_blocking], but fails with a timeout error if no event is received within
    /// `idle_timeout`. The timeout is restarted after each received event, so long transfers are not cut short.
    pub fn recv_all_bytes_timeout_blocking(
        &mut self,
        idle_timeout: Duration,
    ) -> Result<Vec<u8>, StreamError> {
        let mut bytes = vec![];
        let mut deadline = Instant::now() + idle_timeout;
        loop {
            match self.rx.try_recv() {
                Ok(ev) => {
                    if recv_all_inner(ev, &mut bytes)?.is_break() {
                        return Ok(bytes);
                    }
                    deadline = Instant::now() + idle_timeout;
                }
                Err(TryRecvError::Empty) => {
                    if Instant::now() >= deadline {
                        return Err(Error::Timeout.into());
                    }
                    // tokio's blocking_recv has no timeout variant
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(TryRecvError::Disconnected) => return Err(StreamError::Closed),
            }
        }
    }
}

impl From<SWError> for StreamError {
    fn from(e: SWError) -> Self {
        StreamError::DeserializeError(e)
    }
}

fn recv_all_inner(ev: RawStreamEvent, buf: &mut Vec<u8>) -> Result<ControlFlow<()>, StreamError> {
    match ev {
        RawStreamEvent::Data(b) => {
            buf.extend_from_slice(&b);
            Ok(ControlFlow::Continue(()))
        }
        RawStreamEvent::Connected => Ok(ControlFlow::Continue(())),
        RawStreamEvent::Sideband(StreamSideband::Close) => Ok(ControlFlow::Break(())),
        e => Err(StreamError::UnexpectedEvent(e)),
    }
}

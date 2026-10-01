//! Request context passed to every server handler, and the means to send events (stream updates, property
//! change notifications, deferred replies) from handlers and from the rest of the event loop.

use core::future::Future;

use shrink_wrap::prelude::{BufWriter, SerializeShrinkWrap, ShrinkWrapError};

/// Sends serialized messages (`ww_client_server::Event`s) out, e.g., into a framer of a link.
pub trait MessageSink {
    fn send(&mut self, message: &[u8]) -> impl Future<Output = Result<(), ()>>;
}

/// Same as [MessageSink], for code without async (`use_async = false` servers, blocking event loops).
pub trait BlockingMessageSink {
    // same as MessageSink: the sink logs why it failed, callers only need to know that it did
    #[allow(clippy::result_unit_err)]
    fn send(&mut self, message: &[u8]) -> Result<(), ()>;
}

impl<S: MessageSink + ?Sized> MessageSink for &mut S {
    fn send(&mut self, message: &[u8]) -> impl Future<Output = Result<(), ()>> {
        (**self).send(message)
    }
}

impl<S: BlockingMessageSink + ?Sized> BlockingMessageSink for &mut S {
    fn send(&mut self, message: &[u8]) -> Result<(), ()> {
        (**self).send(message)
    }
}

/// Why an event was not sent.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub enum SendError {
    /// Serialization failed, most likely the event does not fit into the scratch buffer or into what is left of
    /// an [EventQueue].
    Serialize(ShrinkWrapError),
    /// The sink did not accept the message (link is down, message is too big, ...), the sink logs why.
    Sink,
}

impl From<ShrinkWrapError> for SendError {
    fn from(e: ShrinkWrapError) -> Self {
        SendError::Serialize(e)
    }
}

/// Where async code sends events to. Implemented by [EventWriter] (what `ww_device::Server` hands out), and
/// by [Context], so that generated `stream_data_ser()` `<name>_send` methods work the same inside and outside
/// of handlers.
pub trait EventOut {
    /// Serialize an event with `ser` into a scratch buffer and send it. `ser` returns the length of the
    /// serialized event, which must start at the beginning of the buffer it is given.
    fn send_with<F>(&mut self, ser: F) -> impl Future<Output = Result<(), SendError>>
    where
        F: FnOnce(&mut [u8]) -> Result<usize, ShrinkWrapError>;

    /// Serialize and send a whole event (usually `ww_client_server::Event`).
    fn send_event<T: SerializeShrinkWrap>(
        &mut self,
        event: &T,
    ) -> impl Future<Output = Result<(), SendError>> {
        self.send_with(|scratch| ser_into(event, scratch))
    }
}

/// Same as [EventOut], for code without async (`use_async = false` servers, blocking event loops).
/// Generated `stream_data_ser()` methods are `<name>_send_blocking`.
pub trait BlockingEventOut {
    /// Serialize an event with `ser` into a scratch buffer and send it. `ser` returns the length of the
    /// serialized event, which must start at the beginning of the buffer it is given.
    fn send_with_blocking<F>(&mut self, ser: F) -> Result<(), SendError>
    where
        F: FnOnce(&mut [u8]) -> Result<usize, ShrinkWrapError>;

    /// Serialize and send a whole event (usually `ww_client_server::Event`).
    fn send_event_blocking<T: SerializeShrinkWrap>(&mut self, event: &T) -> Result<(), SendError> {
        self.send_with_blocking(|scratch| ser_into(event, scratch))
    }
}

fn ser_into<T: SerializeShrinkWrap>(
    value: &T,
    scratch: &mut [u8],
) -> Result<usize, ShrinkWrapError> {
    let mut wr = BufWriter::new(scratch);
    value.ser_shrink_wrap(&mut wr)?;
    Ok(wr.finish()?.len())
}

/// A [MessageSink] with a scratch buffer to serialize events into.
pub struct EventWriter<'s, S> {
    pub sink: S,
    pub scratch: &'s mut [u8],
}

impl<'s, S> EventWriter<'s, S> {
    pub fn new(sink: S, scratch: &'s mut [u8]) -> Self {
        EventWriter { sink, scratch }
    }
}

impl<S: MessageSink> EventOut for EventWriter<'_, S> {
    async fn send_with<F>(&mut self, ser: F) -> Result<(), SendError>
    where
        F: FnOnce(&mut [u8]) -> Result<usize, ShrinkWrapError>,
    {
        let len = ser(self.scratch)?;
        self.sink
            .send(&self.scratch[..len])
            .await
            .map_err(|_| SendError::Sink)
    }
}

impl<S: BlockingMessageSink> BlockingEventOut for EventWriter<'_, S> {
    fn send_with_blocking<F>(&mut self, ser: F) -> Result<(), SendError>
    where
        F: FnOnce(&mut [u8]) -> Result<usize, ShrinkWrapError>,
    {
        let len = ser(self.scratch)?;
        self.sink
            .send(&self.scratch[..len])
            .map_err(|_| SendError::Sink)
    }
}

impl<S: MessageSink> EventWriter<'_, S> {
    /// Run synchronous code that sends events (e.g., a server generated with `use_async = false`) from async
    /// code: events are queued in the scratch buffer and sent after `f` returns, in order. Events that do not
    /// fit fail with [SendError::Serialize], events the sink refuses are dropped (the sink logs why).
    ///
    /// ```ignore
    /// impl WireWeaverAsyncApiBackend for ServerState {
    ///     type Medium = ();
    ///     async fn process_bytes<'a>(&mut self, out: &mut EventWriter<'_, impl MessageSink>, medium: (), data: &[u8], scratch: &'a mut [u8]) -> Result<&'a [u8], ShrinkWrapError> {
    ///         out.queued(|queue| self.process_request_bytes(data, scratch, queue, medium)).await
    ///     }
    ///     // ...
    /// }
    /// ```
    pub async fn queued<R>(&mut self, f: impl FnOnce(&mut EventQueue<'_>) -> R) -> R {
        let mut queue = EventQueue::new(self.scratch);
        let r = f(&mut queue);
        for message in queue.iter() {
            _ = self.sink.send(message).await;
        }
        r
    }
}

const QUEUE_LEN_BYTES: usize = 4;

/// Events serialized one after another into one buffer, to be sent later, see [EventWriter::queued].
pub struct EventQueue<'b> {
    buf: &'b mut [u8],
    len: usize,
}

impl<'b> EventQueue<'b> {
    pub fn new(buf: &'b mut [u8]) -> Self {
        EventQueue { buf, len: 0 }
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }

    /// Queued events, in order.
    pub fn iter(&self) -> impl Iterator<Item = &[u8]> {
        let mut rest = &self.buf[..self.len];
        core::iter::from_fn(move || {
            let (len, tail) = rest.split_first_chunk::<QUEUE_LEN_BYTES>()?;
            let (message, tail) = tail.split_at(u32::from_le_bytes(*len) as usize);
            rest = tail;
            Some(message)
        })
    }
}

impl BlockingEventOut for EventQueue<'_> {
    fn send_with_blocking<F>(&mut self, ser: F) -> Result<(), SendError>
    where
        F: FnOnce(&mut [u8]) -> Result<usize, ShrinkWrapError>,
    {
        let Some((len, free)) = self.buf[self.len..].split_first_chunk_mut::<QUEUE_LEN_BYTES>()
        else {
            return Err(SendError::Serialize(
                ShrinkWrapError::OutOfBoundsWriteRawSlice,
            ));
        };
        let message_len = ser(free)?;
        *len = (message_len as u32).to_le_bytes();
        self.len += QUEUE_LEN_BYTES + message_len;
        Ok(())
    }
}

/// Passed to every server handler: who asked and where to send events to.
///
/// `O` is [EventOut] for servers generated with `use_async = true` and [BlockingEventOut] otherwise,
/// `M` is the medium type given with `ww_codegen!(.., medium = "path::to::Medium")`, `()` by default.
pub struct Context<'c, O, M = ()> {
    seq: u32,
    medium: M,
    out: &'c mut O,
}

impl<'c, O, M: Copy> Context<'c, O, M> {
    pub fn new(seq: u32, medium: M, out: &'c mut O) -> Self {
        Context { seq, medium, out }
    }

    /// Sequence number of the request being served, 0 if the client does not expect a reply.
    pub fn seq(&self) -> u32 {
        self.seq
    }

    /// Medium the request came from (USB, CAN, ...), as passed to the server's `process_request_bytes`.
    pub fn medium(&self) -> M {
        self.medium
    }

    /// Where to send a deferred reply to, None if the client does not expect a reply.
    pub fn reply_to(&self) -> Option<ReplyTo<M>> {
        (self.seq != 0).then_some(ReplyTo {
            medium: self.medium,
            seq: self.seq,
        })
    }

    pub fn out(&mut self) -> &mut O {
        self.out
    }
}

impl<O: EventOut, M> EventOut for Context<'_, O, M> {
    fn send_with<F>(&mut self, ser: F) -> impl Future<Output = Result<(), SendError>>
    where
        F: FnOnce(&mut [u8]) -> Result<usize, ShrinkWrapError>,
    {
        self.out.send_with(ser)
    }
}

impl<O: BlockingEventOut, M> BlockingEventOut for Context<'_, O, M> {
    fn send_with_blocking<F>(&mut self, ser: F) -> Result<(), SendError>
    where
        F: FnOnce(&mut [u8]) -> Result<usize, ShrinkWrapError>,
    {
        self.out.send_with_blocking(ser)
    }
}

/// Request to answer later, from [Context::reply_to]. Pass `seq` to a generated `<method>_send_return`
/// through the server of `medium`.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
pub struct ReplyTo<M = ()> {
    pub medium: M,
    pub seq: u32,
}

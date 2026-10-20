//! Opening a stream by calling its stream function (API-31, CLT-23): `RequestKind::StreamOpen`, answered with
//! `StreamOpened` and the function's result. See `docs/api/stream_reliability.md`.

use std::fmt::Debug;
use std::marker::PhantomData;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::oneshot;
use wire_weaver::prelude::{DeserializeShrinkWrapOwned, UNib32, UVlq32Backfill};
use wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned;
use wire_weaver::shrink_wrap::{
    BufReader, ElementSize, Error as ShrinkWrapError, SerializeShrinkWrapOwned,
};
use ww_client_server::stream::StreamOpenedOwned;
use ww_client_server::{PathKindOwned, RequestKindOwned, RequestOwned};
use ww_stream::{Opened, StreamCursor};

use crate::client::batch::{Unpack, Unpacker};
use crate::client::positioned::{AckMode, PositionedConfig, PositionedStream, Shared};
use crate::error_list::{self, ErrorList};
use crate::event_loop::command::{Command, StreamOpenHook};
use crate::event_loop::commander::TransportCommander;
use crate::event_loop::rx_dispatcher::{ResponseReceiver, StreamUpdateReceiver};
use crate::{Error, Stream};
use std::collections::VecDeque;

/// Result type of a stream function: `()` for `stream!` and functions without a result, `Result<A, E>` for
/// fallible ones. The stream is open only when it is `Ok`.
pub trait StreamOpenResult: DeserializeShrinkWrapOwned + Sized {
    /// What opening returns with the stream handle `S`: `S` for a stream without a result (`stream!`), or
    /// `Result<(A, S), E>`.
    type With<S>;
    /// Whether the device opened the stream.
    fn is_open(&self) -> bool;
    /// Join with the stream handle (dropped when not open).
    fn with<S>(self, stream: S) -> Self::With<S>;
}

impl StreamOpenResult for () {
    type With<S> = S;

    fn is_open(&self) -> bool {
        true
    }

    fn with<S>(self, stream: S) -> S {
        stream
    }
}

impl<A: DeserializeShrinkWrapOwned, E: DeserializeShrinkWrapOwned> StreamOpenResult
    for Result<A, E>
{
    type With<S> = Result<(A, S), E>;

    fn is_open(&self) -> bool {
        self.is_ok()
    }

    fn with<S>(self, stream: S) -> Result<(A, S), E> {
        self.map(|a| (a, stream))
    }
}

/// A stream function's result as sent, for the dynamic client, which decodes it from the introspected type; see
/// `DynResource::positioned_stream`.
#[doc(hidden)]
pub struct RawOpenResult(pub Vec<u8>);

impl DeserializeShrinkWrapOwned for RawOpenResult {
    const ELEMENT_SIZE: ElementSize = ElementSize::Unsized;

    fn des_shrink_wrap_owned(rd: &mut BufReader<'_>) -> Result<Self, ShrinkWrapError> {
        Ok(RawOpenResult(TailBytesOwned::des_shrink_wrap_owned(rd)?.0))
    }
}

impl StreamOpenResult for RawOpenResult {
    type With<S> = (RawOpenResult, S);

    /// Decided by [PreparedStreamOpen::raw_fallible] instead.
    fn is_open(&self) -> bool {
        true
    }

    fn with<S>(self, stream: S) -> (RawOpenResult, S) {
        (self, stream)
    }
}

mod sealed {
    pub trait Sealed {}
}

/// A stream handle that opening a stream returns: [Stream] for plain streams, [PositionedStream] for counted and
/// reliable ones.
pub trait StreamHandle: sealed::Sealed + Sized {
    /// What the handle receives.
    type Item;
    #[doc(hidden)]
    fn positioned() -> bool;
    #[doc(hidden)]
    fn from_parts(parts: HandleParts, unpack: Option<Unpack<Self::Item>>) -> Self;
}

#[doc(hidden)]
pub struct HandleParts(Parts);

enum Parts {
    Plain {
        transport_cmd_tx: TransportCommander,
        path_kind: PathKindOwned,
        rx: StreamUpdateReceiver,
    },
    Positioned {
        transport_cmd_tx: TransportCommander,
        path_kind: PathKindOwned,
        shared: Arc<Shared>,
    },
}

impl<T> sealed::Sealed for Stream<T> {}
impl<T> StreamHandle for Stream<T> {
    type Item = T;

    fn positioned() -> bool {
        false
    }

    fn from_parts(parts: HandleParts, unpack: Option<Unpack<T>>) -> Self {
        let Parts::Plain {
            transport_cmd_tx,
            path_kind,
            rx,
        } = parts.0
        else {
            unreachable!("checked when opening")
        };
        Stream {
            transport_cmd_tx,
            path_kind,
            rx,
            unpacker: Unpacker::new(unpack),
            pending: VecDeque::new(),
            _phantom: PhantomData,
        }
    }
}

impl<T> sealed::Sealed for PositionedStream<T> {}
impl<T> StreamHandle for PositionedStream<T> {
    type Item = T;

    fn positioned() -> bool {
        true
    }

    fn from_parts(parts: HandleParts, unpack: Option<Unpack<T>>) -> Self {
        let Parts::Positioned {
            transport_cmd_tx,
            path_kind,
            shared,
        } = parts.0
        else {
            unreachable!("checked when opening")
        };
        PositionedStream {
            shared,
            transport_cmd_tx,
            path_kind,
            run: None,
            unpacker: Unpacker::new(unpack),
            pending: VecDeque::new(),
            _phantom: PhantomData,
        }
    }
}

/// Opening a stream, from `Commander::prepare_stream_open` (`device.samples(config)` in generated code) or
/// `Commander::prepare_stream_resume` (`device.samples_resume(cursor)`). Does nothing until
/// [call](Self::call) or [blocking_call](Self::blocking_call).
#[must_use = "PreparedStreamOpen does nothing, unless call() or blocking_call() is used"]
pub struct PreparedStreamOpen<R, S: StreamHandle> {
    pub(crate) postpone_err: Result<(), Error>,
    pub(crate) transport_cmd_tx: TransportCommander,
    pub(crate) path_kind: PathKindOwned,
    pub(crate) delivery: Option<ww_self::Delivery>,
    pub(crate) args: Vec<u8>,
    pub(crate) resume: Option<StreamCursor>,
    /// Batched streams: the most a batch may wait on the device, µs (None: the device's default)
    pub(crate) max_age_us: Option<u32>,
    pub(crate) ack_mode: AckMode,
    pub(crate) window: u32,
    pub(crate) queue_bytes: usize,
    pub(crate) timeout_override: Option<Duration>,
    /// The result is a `Result` the type doesn't know of ([RawOpenResult]): open only when it is `Ok`
    pub(crate) raw_fallible: bool,
    /// `#[errors(..)]` of the stream function (API-32), checked on the device's error, see [Self::with_error_list]
    pub(crate) error_check: Option<ErrorCheck<R>>,
    /// A batched stream's events become items with it (API-41), see [Self::batched]
    pub(crate) unpack: Option<Unpack<S::Item>>,
    pub(crate) _phantom: PhantomData<(R, S)>,
}

/// Checks a stream function's result against its `#[errors(..)]` list.
pub(crate) type ErrorCheck<R> = Box<dyn Fn(&R) + Send + Sync>;

impl<A, E: Debug + 'static, S: StreamHandle> PreparedStreamOpen<Result<A, E>, S> {
    /// The stream function's `#[errors(..)]` list (API-32), used by generated client code: a device error outside
    /// it is logged as a warning (a panic with [set_strict_error_lists](crate::set_strict_error_lists)).
    /// `resource` names the stream function in the message.
    pub fn with_error_list(mut self, list: ErrorList<E>, resource: &'static str) -> Self {
        self.error_check = Some(Box::new(move |result: &Result<A, E>| {
            if let Err(e) = result {
                error_list::check(e, Some(&list), resource);
            }
        }));
        self
    }
}

impl<R: StreamOpenResult, S: StreamHandle> PreparedStreamOpen<R, S> {
    /// Use a provided timeout for the open request instead of the default one.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout_override = Some(timeout);
        self
    }

    /// When a reliable stream's data counts as received, so the device frees it ([AckMode::OnReceive] by default).
    pub fn ack_mode(mut self, mode: AckMode) -> Self {
        self.ack_mode = mode;
        self
    }

    /// Reliable streams: bytes the device may have in flight beyond what the host acknowledged, which is what the
    /// host queues for the stream at most ([DEFAULT_WINDOW](crate::DEFAULT_WINDOW)).
    pub fn window(mut self, bytes: u32) -> Self {
        self.window = bytes;
        self
    }

    /// Plain and counted streams: bytes of items queued before new ones are dropped (and reported as a
    /// `Gap { cause: Host }`), [ClientConfig::stream_queue_bytes](crate::ClientConfig::stream_queue_bytes) by
    /// default.
    pub fn queue_bytes(mut self, bytes: usize) -> Self {
        self.queue_bytes = bytes;
        self
    }

    /// The stream is batched (API-41): each event is a batch of items, `unpack` ([unpack_timed](crate::unpack_timed)
    /// or [unpack_untimed](crate::unpack_untimed)) decodes one, and the handle gives them one by one. Used by
    /// generated client code.
    pub fn batched(mut self, unpack: Unpack<S::Item>) -> Self {
        self.unpack = Some(unpack);
        self
    }

    /// Batched streams (API-41): the most a batch may wait on the device before it is sent, the latency the host
    /// accepts for fewer, bigger events; the device's default without it. The device clamps it to what it supports
    /// (rounded to microseconds, at most `u32::MAX` µs). Ignored by streams that aren't batched.
    pub fn max_age(mut self, max_age: Duration) -> Self {
        self.max_age_us = Some(u32::try_from(max_age.as_micros()).unwrap_or(u32::MAX));
        self
    }

    /// Open the stream: the stream function's result with the stream handle (see [StreamOpenResult]).
    pub async fn call(mut self) -> Result<R::With<S>, Error> {
        std::mem::replace(&mut self.postpone_err, Ok(()))?;
        let plain_rx = if S::positioned() || self.delivery.is_some() {
            None
        } else {
            Some(
                self.transport_cmd_tx
                    .register_stream(&self.path_kind, self.queue_bytes)
                    .await?,
            )
        };
        let open_check = self.open_check();
        let (parts, (cmd, done_rx)) = self.start(plain_rx)?;
        self.transport_cmd_tx.send_command(cmd).await?;
        let response = done_rx.await.map_err(|_| Error::RxDispatcherNotRunning);
        finish::<R, S>(
            parts,
            response,
            open_check,
            self.error_check.as_ref(),
            self.unpack,
        )
    }

    /// Blocking variant of [Self::call].
    pub fn blocking_call(mut self) -> Result<R::With<S>, Error> {
        std::mem::replace(&mut self.postpone_err, Ok(()))?;
        let plain_rx = if S::positioned() || self.delivery.is_some() {
            None
        } else {
            Some(
                self.transport_cmd_tx
                    .register_stream_blocking(&self.path_kind, self.queue_bytes)?,
            )
        };
        let open_check = self.open_check();
        let (parts, (cmd, done_rx)) = self.start(plain_rx)?;
        self.transport_cmd_tx.send_command_blocking(cmd)?;
        let response = done_rx
            .blocking_recv()
            .map_err(|_| Error::RxDispatcherNotRunning);
        finish::<R, S>(
            parts,
            response,
            open_check,
            self.error_check.as_ref(),
            self.unpack,
        )
    }

    /// Checks, the open request and what it needs (a plain stream's receiver, registered before the request goes
    /// out so that nothing the device sends after opening is lost; a positioned stream's state).
    fn start(
        &self,
        plain_rx: Option<StreamUpdateReceiver>,
    ) -> Result<(Parts, (Command, ResponseReceiver)), Error> {
        if S::positioned() != self.delivery.is_some() {
            return Err(Error::User(format!(
                "stream at {:?} is {}, opened as {}",
                self.path_kind,
                match self.delivery {
                    Some(d) => format!("{d:?}"),
                    None => "plain".into(),
                },
                if S::positioned() {
                    "a PositionedStream"
                } else {
                    "a plain Stream"
                },
            )));
        }
        let (done_tx, done_rx) = oneshot::channel();
        let request = RequestOwned {
            seq: UVlq32Backfill(0),
            path_kind: self.path_kind.clone(),
            kind: RequestKindOwned::StreamOpen {
                codecs: None,
                resume: self.resume,
                max_age_us: self.max_age_us.map(UNib32),
                args: TailBytesOwned(self.args.clone()),
            },
        }
        .to_ww_bytes_owned()?;
        let timeout = self
            .timeout_override
            .unwrap_or(self.transport_cmd_tx.default_timeout());
        let Some(delivery) = self.delivery else {
            let cmd = Command::SendMessage {
                bytes: request,
                done_tx: Some((done_tx, timeout)),
            };
            let parts = Parts::Plain {
                transport_cmd_tx: self.transport_cmd_tx.clone(),
                path_kind: self.path_kind.clone(),
                rx: plain_rx.expect("registered for a plain stream"),
            };
            return Ok((parts, (cmd, done_rx)));
        };
        let PathKindOwned::Absolute { path } = &self.path_kind else {
            // TODO(CLT-23): events carry the absolute path, a trait path has to be resolved first
            return Err(Error::User(
                "counted and reliable streams can only be opened on an absolute path yet".into(),
            ));
        };
        let shared = Shared::new(
            path.clone(),
            PositionedConfig {
                delivery,
                ack_mode: self.ack_mode,
                window: self.window,
                queue_bytes: self.queue_bytes,
                max_age_us: self.max_age_us,
            },
        );
        let cmd = Command::OpenStream {
            bytes: request,
            done_tx,
            timeout,
            hook: Box::new(StreamOpenHook {
                shared: shared.clone(),
                resume: self.resume,
                is_open: self.open_check(),
            }),
        };
        let parts = Parts::Positioned {
            transport_cmd_tx: self.transport_cmd_tx.clone(),
            path_kind: self.path_kind.clone(),
            shared,
        };
        Ok((parts, (cmd, done_rx)))
    }

    /// Whether an answer to the open says the stream is open.
    fn open_check(&self) -> fn(&[u8]) -> bool {
        match self.raw_fallible {
            true => is_ok_result,
            false => is_open::<R>,
        }
    }
}

/// Decode the device's answer to an open: `StreamOpened` with the stream function's result in it.
pub(crate) fn decode_opened(bytes: &[u8]) -> Result<(Opened, Vec<u8>), Error> {
    let opened = StreamOpenedOwned::from_ww_bytes_owned(bytes)?;
    let result = opened.result.0.clone();
    Ok((
        Opened {
            session: opened.session.0,
            oldest: opened.oldest.0,
            live_from: opened.live_from.0,
            resume_from: opened.resume_from.map(|p| p.0),
        },
        result,
    ))
}

/// For the event loop: whether an answer to an open says the stream is open (decodes `R`).
pub(crate) fn is_open<R: StreamOpenResult>(bytes: &[u8]) -> bool {
    decode_opened(bytes)
        .and_then(|(_, result)| Ok(R::from_ww_bytes_owned(&result)?))
        .is_ok_and(|r| r.is_open())
}

/// Whether the result in an answer to an open is an `Ok` (a `Result` starts with its is-ok flag).
fn is_ok_result(bytes: &[u8]) -> bool {
    decode_opened(bytes).is_ok_and(|(_, result)| BufReader::new(&result).read_bool() == Ok(true))
}

fn finish<R: StreamOpenResult, S: StreamHandle>(
    parts: Parts,
    response: Result<Result<Vec<u8>, Error>, Error>,
    open_check: fn(&[u8]) -> bool,
    error_check: Option<&ErrorCheck<R>>,
    unpack: Option<Unpack<S::Item>>,
) -> Result<R::With<S>, Error> {
    let decoded = response.and_then(|r| r).and_then(|bytes| {
        let (_, result) = decode_opened(&bytes)?;
        Ok((R::from_ww_bytes_owned(&result)?, open_check(&bytes)))
    });
    if let (Ok((r, _)), Some(check)) = (&decoded, error_check) {
        check(r);
    }
    let result = match decoded {
        Ok((r, true)) => {
            if let Parts::Positioned {
                transport_cmd_tx,
                shared,
                ..
            } = &parts
            {
                let mut streams = transport_cmd_tx
                    .streams
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                streams.retain(|s| s.strong_count() > 0);
                streams.push(Arc::downgrade(shared));
            }
            r
        }
        Ok((r, false)) => {
            discard(&parts);
            return Ok(r.with(S::from_parts(HandleParts(parts), unpack)));
        }
        Err(e) => {
            discard(&parts);
            return Err(e);
        }
    };
    Ok(result.with(S::from_parts(HandleParts(parts), unpack)))
}

/// The stream didn't open: its handle goes without closing it on the device.
fn discard(parts: &Parts) {
    if let Parts::Positioned { shared, .. } = parts {
        shared.lock().closed = true;
    }
}

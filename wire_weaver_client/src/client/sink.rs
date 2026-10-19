use crate::event_loop::command::Command;
use crate::event_loop::commander::TransportCommander;
use crate::event_loop::event_queue::TryRecvError;
use crate::event_loop::rx_dispatcher::{ResponseReceiver, StreamUpdateReceiver};
use crate::{Error, RawStreamEvent, StreamError};
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::num::NonZeroU32;
use std::time::{Duration, Instant};
use tokio::sync::oneshot;
use wire_weaver::shrink_wrap::{DeserializeShrinkWrap, SerializeShrinkWrap};
use wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned;
use ww_client_server::stream::StreamOpened;
use ww_client_server::{PathKindOwned, StreamSideband};
use ww_stream::sink::{SinkConfig, SinkDue, SinkOpenError, SinkSideband, SinkTx, SinkTxError};
use ww_stream::{Opened, StreamCursor};

/// Timing of a credit sink, tune it to the link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreditConfig {
    /// Send `Query` when out of credit (or waiting in `flush()`) and nothing came from the device for this long, and
    /// the open again when it wasn't answered. Should be longer than the link's round trip.
    pub query_interval: Duration,
    /// Fail with [crate::Error::Timeout] when the device made no progress for this long (no new credit, nothing accepted),
    /// longer than its worst stall (e.g., a flash sector erase). None waits forever.
    pub stall_timeout: Option<Duration>,
}

impl Default for CreditConfig {
    fn default() -> Self {
        let config = SinkConfig::default();
        CreditConfig {
            query_interval: Duration::from_millis(config.query_interval_ms),
            stall_timeout: config.stall_timeout_ms.map(Duration::from_millis),
        }
    }
}

impl From<CreditConfig> for SinkConfig {
    fn from(config: CreditConfig) -> Self {
        SinkConfig {
            query_interval_ms: config.query_interval.as_millis() as u64,
            stall_timeout_ms: config.stall_timeout.map(|t| t.as_millis() as u64),
        }
    }
}

/// Stream of typed values from host to device.
/// Also holds a sideband channel, in both directions.
///
/// A sink marked `#[credit]` in the API has credit-based flow control (`docs/api/credit_sinks.md`): `send()` waits
/// until the device has room for all bytes of the value, `flush()` waits until it accepted everything. A value
/// bigger than the device's receive buffer fails with [StreamError::ItemTooBig]. Positions count units: one per
/// value, the bytes of the payload on a byte sink, or what [Self::send_units] names.
pub struct Sink<T> {
    transport_cmd_tx: TransportCommander,
    path_kind: PathKindOwned,
    sideband_rx: StreamUpdateReceiver,
    /// Events received while waiting for credit, for the user
    pending: VecDeque<RawStreamEvent>,
    credit: Option<Credit>,
    _phantom: PhantomData<T>,
}

/// Host side of a credit sink: [SinkTx] and what drives it.
struct Credit {
    tx: SinkTx,
    /// Time base of the ms that [SinkTx] counts
    epoch: Instant,
    /// The answer to the last `StreamOpen` sent, until it came. Sideband events stay queued meanwhile: an `Ack`
    /// before the answer would be taken as one of an earlier session.
    opening: Option<ResponseReceiver>,
    /// The device closed the session (it lost it): open it again with this cursor
    reopen: Option<StreamCursor>,
    /// The link went down during the session, the next call fails once
    lost: bool,
}

impl Credit {
    fn now_ms(&self) -> u64 {
        self.epoch.elapsed().as_millis() as u64
    }
}

/// What a sink is waiting for.
#[derive(Debug, Clone, Copy)]
enum Goal<'a> {
    /// The device answered the open
    Opened,
    /// A new item went out
    Send { data: &'a [u8], units: u64 },
    /// The device accepted everything
    Flush,
}

/// Next step towards a [Goal].
enum Step {
    /// Send this, then go on
    Send(Command),
    /// Send this and the goal is reached (the new item)
    SendLast(Command),
    /// Nothing to send now: wait this long for an event
    Wait(Duration),
    Done,
}

/// A new session id: random, never 0 (the host names a credit sink's session, API-34).
fn new_session() -> u32 {
    use std::hash::{BuildHasher, RandomState};
    loop {
        let id = RandomState::new().hash_one(std::time::SystemTime::now()) as u32;
        if id != 0 {
            return id;
        }
    }
}

fn tx_err(e: SinkTxError) -> StreamError {
    match e {
        SinkTxError::ItemTooBig { len, buffer } => StreamError::ItemTooBig { len, buffer },
        SinkTxError::BadUnits { units } => StreamError::BadUnits { units },
    }
}

impl<T> Sink<T> {
    pub(crate) fn new(
        transport_cmd_tx: TransportCommander,
        path_kind: PathKindOwned,
        sideband_rx: StreamUpdateReceiver,
        credit: bool,
    ) -> Self {
        Sink {
            transport_cmd_tx,
            path_kind,
            sideband_rx,
            pending: VecDeque::new(),
            credit: credit.then(|| Credit {
                tx: SinkTx::new(SinkConfig::default()),
                epoch: Instant::now(),
                opening: None,
                reopen: None,
                lost: false,
            }),
            _phantom: PhantomData,
        }
    }

    /// Open the sink. A credit sink starts a new session (the host names it) and returns once the device answered;
    /// on a plain sink this sends the `Open` command through the sideband channel, for the device's sideband handler.
    pub async fn open(&mut self) -> Result<(), StreamError> {
        if self.credit.is_none() {
            return self.sideband(StreamSideband::Open).await;
        }
        let cmd = self.start_session()?;
        self.transport_cmd_tx.send_command(cmd).await?;
        self.drive(Goal::Opened).await
    }

    /// Blocking [Self::open].
    pub fn open_blocking(&mut self) -> Result<(), StreamError> {
        if self.credit.is_none() {
            return self.sideband_blocking(StreamSideband::Open);
        }
        let cmd = self.start_session()?;
        self.transport_cmd_tx.send_command_blocking(cmd)?;
        self.drive_blocking(Goal::Opened)
    }

    /// [Self::open] of a credit sink with the session named by user code instead of a random one, e.g., from a hash
    /// of the data it carries. The device keeps a session it is in: opening the one it has continues where it is
    /// (also after a restart of the host), any other starts at 0. Does nothing more than [Self::open] on a plain
    /// sink.
    pub async fn open_session(&mut self, session: NonZeroU32) -> Result<(), StreamError> {
        if self.credit.is_none() {
            return self.open().await;
        }
        let cmd = self.start_named_session(session.get())?;
        self.transport_cmd_tx.send_command(cmd).await?;
        self.drive(Goal::Opened).await
    }

    /// Blocking [Self::open_session].
    pub fn open_session_blocking(&mut self, session: NonZeroU32) -> Result<(), StreamError> {
        if self.credit.is_none() {
            return self.open_blocking();
        }
        let cmd = self.start_named_session(session.get())?;
        self.transport_cmd_tx.send_command_blocking(cmd)?;
        self.drive_blocking(Goal::Opened)
    }

    /// Send Close command through the sideband channel.
    /// On a credit sink, waits until the device accepted everything first, see [Self::flush].
    pub async fn close(&mut self) -> Result<(), StreamError> {
        if self.in_session() {
            self.flush().await?;
        }
        if let Some(credit) = &mut self.credit {
            credit.tx.closed();
            credit.opening = None;
            credit.reopen = None;
        }
        self.sideband(StreamSideband::Close).await
    }

    /// Send Close command through the sideband channel.
    /// On a credit sink, waits until the device accepted everything first, see [Self::flush_blocking].
    pub fn close_blocking(&mut self) -> Result<(), StreamError> {
        if self.in_session() {
            self.flush_blocking()?;
        }
        if let Some(credit) = &mut self.credit {
            credit.tx.closed();
            credit.opening = None;
            credit.reopen = None;
        }
        self.sideband_blocking(StreamSideband::Close)
    }

    /// Send command through the sideband channel
    pub async fn sideband(&self, sideband: StreamSideband) -> Result<(), StreamError> {
        self.transport_cmd_tx
            .send_stream_sideband(self.path_kind.clone(), sideband, None)
            .await?;
        Ok(())
    }

    /// Send command through the sideband channel
    pub fn sideband_blocking(&self, sideband: StreamSideband) -> Result<(), StreamError> {
        self.transport_cmd_tx.send_stream_sideband_blocking(
            self.path_kind.clone(),
            sideband,
            None,
        )?;
        Ok(())
    }

    /// Receive one sideband event from the device: a reply to [Self::sideband] or one sent by the device on its own.
    /// Skip [RawStreamEvent::Connected] events. Credit events of a credit sink are used up internally and not returned.
    /// Returns an error if a Disconnected event is received instead.
    ///
    /// See [Self::recv_sideband_blocking] for a blocking variant of this method.
    pub async fn recv_sideband(&mut self) -> Result<StreamSideband, StreamError> {
        loop {
            let ev = self.recv_any().await?;
            if let Some(sideband) = sideband_or_err(ev)? {
                return Ok(sideband);
            }
        }
    }

    /// Receive one sideband event from the device in a blocking manner.
    /// Skip [RawStreamEvent::Connected] events. Credit events of a credit sink are used up internally and not returned.
    /// Returns an error if a Disconnected event is received instead.
    ///
    /// See [Self::recv_sideband] for an asynchronous variant of this method.
    pub fn recv_sideband_blocking(&mut self) -> Result<StreamSideband, StreamError> {
        loop {
            let ev = self.recv_any_blocking()?;
            if let Some(sideband) = sideband_or_err(ev)? {
                return Ok(sideband);
            }
        }
    }

    /// Try to receive one sideband event from the device, `None` if there is none yet.
    /// Skip [RawStreamEvent::Connected] events. Credit events of a credit sink are used up internally and not returned.
    /// Returns an error if a Disconnected event is received instead.
    pub fn try_recv_sideband(&mut self) -> Result<Option<StreamSideband>, StreamError> {
        loop {
            match self.try_recv_any()? {
                Some(ev) => {
                    if let Some(sideband) = sideband_or_err(ev)? {
                        return Ok(Some(sideband));
                    }
                }
                None => return Ok(None),
            }
        }
    }

    /// Receive one event of any kind: sideband, Connected or Disconnected.
    ///
    /// See [Self::recv_any_blocking] for a blocking variant of this method.
    pub async fn recv_any(&mut self) -> Result<RawStreamEvent, StreamError> {
        loop {
            if let Some(ev) = self.pending.pop_front() {
                return Ok(ev);
            }
            let ev = self.sideband_rx.recv().await.ok_or(StreamError::Closed)?;
            self.filter(ev);
        }
    }

    /// Receive one event of any kind: sideband, Connected or Disconnected.
    ///
    /// See [Self::recv_any] for an asynchronous variant of this method.
    pub fn recv_any_blocking(&mut self) -> Result<RawStreamEvent, StreamError> {
        loop {
            if let Some(ev) = self.pending.pop_front() {
                return Ok(ev);
            }
            let ev = self
                .sideband_rx
                .blocking_recv()
                .ok_or(StreamError::Closed)?;
            self.filter(ev);
        }
    }

    fn try_recv_any(&mut self) -> Result<Option<RawStreamEvent>, StreamError> {
        self.drain()?;
        Ok(self.pending.pop_front())
    }

    /// True for a sink with credit-based flow control (`#[credit]`).
    pub fn is_credit(&self) -> bool {
        self.credit.is_some()
    }

    /// Timing of a credit sink, see [CreditConfig]. Does nothing on other sinks.
    pub fn set_credit_config(&mut self, config: CreditConfig) {
        if let Some(credit) = &mut self.credit {
            credit.tx.config = config.into();
        }
    }

    /// Units the device accepted and units sent in this session of a credit sink (one per value, the bytes of a
    /// byte sink's payloads), e.g., for a progress bar.
    pub fn credit_progress(&self) -> Option<(u64, u64)> {
        self.credit.as_ref().map(|c| (c.tx.acked(), c.tx.sent()))
    }

    /// Wait until the device accepted everything sent so far (credit sinks), resending what was lost.
    /// Returns right away for other sinks, or when nothing was sent.
    ///
    /// See [Self::flush_blocking] for a blocking variant of this method.
    pub async fn flush(&mut self) -> Result<(), StreamError> {
        if !self.in_session() {
            return Ok(());
        }
        if let Some(query) = self.query_now() {
            self.sideband(query).await?;
        }
        self.drive(Goal::Flush).await
    }

    /// Wait until the device accepted everything sent so far (credit sinks), resending what was lost.
    /// Returns right away for other sinks, or when nothing was sent.
    ///
    /// See [Self::flush] for an asynchronous variant of this method.
    pub fn flush_blocking(&mut self) -> Result<(), StreamError> {
        if !self.in_session() {
            return Ok(());
        }
        if let Some(query) = self.query_now() {
            self.sideband_blocking(query)?;
        }
        self.drive_blocking(Goal::Flush)
    }

    /// Non-blocking [Self::flush] for promise-style code polling it: true once the device accepted everything.
    pub fn try_flush(&mut self) -> Result<bool, StreamError> {
        if !self.in_session() {
            return Ok(true);
        }
        self.drive_try(Goal::Flush)
    }

    /// A credit sink in a session.
    fn in_session(&self) -> bool {
        self.credit
            .as_ref()
            .is_some_and(|c| c.tx.session().is_some())
    }

    /// A query to send right away, when the device was answered and something isn't acknowledged.
    fn query_now(&mut self) -> Option<StreamSideband> {
        let credit = self.credit.as_mut()?;
        if !credit.tx.is_synced() || credit.tx.all_acked() {
            return None;
        }
        let now = credit.now_ms();
        Some(credit.tx.query_now(now))
    }

    pub(crate) async fn send_raw(&mut self, bytes: Vec<u8>, units: u64) -> Result<(), StreamError> {
        if self.credit.is_none() {
            self.transport_cmd_tx
                .send_write_request_forget(self.path_kind.clone(), bytes)
                .await?;
            return Ok(());
        }
        self.drive(Goal::Send {
            data: &bytes,
            units,
        })
        .await
    }

    pub(crate) fn send_raw_blocking(&mut self, bytes: Vec<u8>, units: u64) -> Result<(), StreamError> {
        if self.credit.is_none() {
            self.transport_cmd_tx
                .send_write_request_forget_blocking(self.path_kind.clone(), bytes)?;
            return Ok(());
        }
        self.drive_blocking(Goal::Send {
            data: &bytes,
            units,
        })
    }

    pub(crate) fn try_send_raw(&mut self, bytes: &[u8], units: u64) -> Result<bool, StreamError> {
        if self.credit.is_none() {
            let Some(permit) = self.transport_cmd_tx.try_reserve()? else {
                return Ok(false);
            };
            permit.send(
                self.transport_cmd_tx
                    .write_forget_command(self.path_kind.clone(), bytes.to_vec())?,
            );
            return Ok(true);
        }
        self.drive_try(Goal::Send { data: bytes, units })
    }

    /// Start a new session, returns the `StreamOpen` to send.
    fn start_session(&mut self) -> Result<Command, StreamError> {
        self.start_named_session(new_session())
    }

    /// Start `session` from position 0 on the host, returns the `StreamOpen` to send.
    fn start_named_session(&mut self, session: u32) -> Result<Command, StreamError> {
        let credit = self.credit.as_mut().expect("credit sink");
        let now = credit.now_ms();
        let cursor = credit.tx.open(session, now);
        credit.reopen = None;
        credit.lost = false;
        self.open_command(cursor)
    }

    /// `StreamOpen` with `cursor`, its answer awaited from now on.
    fn open_command(&mut self, cursor: StreamCursor) -> Result<Command, StreamError> {
        let (cmd, rx) = self
            .transport_cmd_tx
            .sink_open_command(self.path_kind.clone(), cursor)?;
        self.credit.as_mut().expect("credit sink").opening = Some(rx);
        Ok(cmd)
    }

    /// The device's answer to the open, or that it didn't come.
    fn on_open_answer(
        &mut self,
        answer: Result<Result<Vec<u8>, Error>, oneshot::error::RecvError>,
    ) -> Result<(), StreamError> {
        let credit = self.credit.as_mut().expect("credit sink");
        credit.opening = None;
        let bytes = match answer {
            Ok(Ok(bytes)) => bytes,
            // lost: opened again when due
            Ok(Err(Error::Timeout)) => return Ok(()),
            Ok(Err(e)) => {
                credit.tx.closed();
                return Err(e.into());
            }
            Err(_) => return Err(Error::EventLoopNotRunning.into()),
        };
        let opened = StreamOpened::from_ww_bytes(&bytes).map_err(Error::from)?;
        let capacity = opened.capacity.map(|c| c.0);
        let opened = Opened::from_wire(&opened);
        let now = credit.now_ms();
        match credit.tx.opened(&opened, capacity, now) {
            Ok(_) | Err(SinkOpenError::Stale) => Ok(()),
            Err(SinkOpenError::SessionLost { device_at, acked }) => {
                Err(StreamError::SinkSessionLost { device_at, acked })
            }
            Err(SinkOpenError::Ahead { device_at, sent }) => {
                Err(StreamError::SinkAhead { device_at, sent })
            }
        }
    }

    /// Take in the answer to the open if it came.
    fn poll_open_answer(&mut self) -> Result<(), StreamError> {
        let Some(rx) = self
            .credit
            .as_mut()
            .and_then(|credit| credit.opening.as_mut())
        else {
            return Ok(());
        };
        match rx.try_recv() {
            Ok(answer) => self.on_open_answer(Ok(answer)),
            Err(oneshot::error::TryRecvError::Empty) => Ok(()),
            Err(oneshot::error::TryRecvError::Closed) => {
                Err(Error::EventLoopNotRunning.into())
            }
        }
    }

    fn is_opening(&self) -> bool {
        self.credit.as_ref().is_some_and(|c| c.opening.is_some())
    }

    /// The next thing to do towards `goal` (sans-IO, for the async, blocking and non-blocking drivers below).
    fn step(&mut self, goal: Goal<'_>) -> Result<Step, StreamError> {
        self.poll_open_answer()?;
        if !self.is_opening() {
            self.drain()?;
        }
        let credit = self.credit.as_mut().expect("credit sink");
        if credit.lost {
            // the device lost the session with the link: values sent so far may be lost too
            credit.lost = false;
            return Err(StreamError::UnexpectedEvent(RawStreamEvent::Disconnected));
        }
        if credit.tx.session().is_none() {
            return match goal {
                Goal::Send { .. } => Ok(Step::Send(self.start_session()?)),
                Goal::Opened | Goal::Flush => Ok(Step::Done),
            };
        }
        if let Some(cursor) = credit.reopen.take() {
            return Ok(Step::Send(self.open_command(cursor)?));
        }
        let path_kind = self.path_kind.clone();
        if let Some(write) = credit.tx.next_resend() {
            return Ok(Step::Send(
                self.transport_cmd_tx.sink_write_command(path_kind, write)?,
            ));
        }
        match goal {
            Goal::Opened if credit.tx.is_synced() => return Ok(Step::Done),
            Goal::Flush if credit.tx.all_acked() => return Ok(Step::Done),
            Goal::Send { data, units } => {
                if let Some(write) = credit.tx.try_new(data, units).map_err(tx_err)? {
                    return Ok(Step::SendLast(
                        self.transport_cmd_tx.sink_write_command(path_kind, write)?,
                    ));
                }
            }
            _ => {}
        }
        let now = credit.now_ms();
        match credit.tx.due(now) {
            None => Ok(Step::Wait(Duration::from_millis(credit.tx.wait_ms(now)))),
            Some(SinkDue::Stalled) => Err(Error::Timeout.into()),
            Some(SinkDue::Open(cursor)) => Ok(Step::Send(self.open_command(cursor)?)),
            Some(SinkDue::Query(query)) => Ok(Step::Send(
                self.transport_cmd_tx
                    .sideband_command(path_kind, query)?,
            )),
        }
    }

    async fn drive(&mut self, goal: Goal<'_>) -> Result<(), StreamError> {
        loop {
            match self.step(goal)? {
                Step::Send(cmd) => self.transport_cmd_tx.send_command(cmd).await?,
                Step::SendLast(cmd) => return Ok(self.transport_cmd_tx.send_command(cmd).await?),
                Step::Wait(wait) => self.wait(wait).await?,
                Step::Done => return Ok(()),
            }
        }
    }

    fn drive_blocking(&mut self, goal: Goal<'_>) -> Result<(), StreamError> {
        loop {
            match self.step(goal)? {
                Step::Send(cmd) => self.transport_cmd_tx.send_command_blocking(cmd)?,
                Step::SendLast(cmd) => {
                    return Ok(self.transport_cmd_tx.send_command_blocking(cmd)?);
                }
                Step::Wait(wait) => self.wait_blocking(wait)?,
                Step::Done => return Ok(()),
            }
        }
    }

    /// Without waiting: true once the goal is reached, false if it needs credit, an answer or room in the command
    /// queue first.
    fn drive_try(&mut self, goal: Goal<'_>) -> Result<bool, StreamError> {
        let transport = self.transport_cmd_tx.clone();
        loop {
            // a place first, so that a step taken is always sent
            let Some(permit) = transport.try_reserve()? else {
                return Ok(false);
            };
            match self.step(goal)? {
                Step::Send(cmd) => permit.send(cmd),
                Step::SendLast(cmd) => {
                    permit.send(cmd);
                    return Ok(true);
                }
                Step::Wait(_) => return Ok(false),
                Step::Done => return Ok(true),
            }
        }
    }

    /// Wait up to `wait` for the answer to the open or else for a sideband event.
    async fn wait(&mut self, wait: Duration) -> Result<(), StreamError> {
        let credit = self.credit.as_mut().expect("credit sink");
        if let Some(rx) = &mut credit.opening {
            if let Ok(answer) = tokio::time::timeout(wait, rx).await {
                self.on_open_answer(answer)?;
            }
            return Ok(());
        }
        if let Ok(ev) = tokio::time::timeout(wait, self.sideband_rx.recv()).await {
            self.filter(ev.ok_or(StreamError::Closed)?);
        }
        Ok(())
    }

    /// Blocking [Self::wait].
    fn wait_blocking(&mut self, wait: Duration) -> Result<(), StreamError> {
        let deadline = Instant::now() + wait;
        loop {
            if self.is_opening() {
                self.poll_open_answer()?;
                if !self.is_opening() {
                    return Ok(());
                }
            } else {
                match self.sideband_rx.try_recv() {
                    Ok(ev) => {
                        self.filter(ev);
                        return Ok(());
                    }
                    Err(TryRecvError::Empty) => {}
                    Err(TryRecvError::Disconnected) => return Err(StreamError::Closed),
                }
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(());
            }
            // tokio's blocking_recv has no timeout variant
            std::thread::sleep((deadline - now).min(Duration::from_millis(1)));
        }
    }

    /// Take in everything received so far without waiting.
    fn drain(&mut self) -> Result<(), StreamError> {
        loop {
            match self.sideband_rx.try_recv() {
                Ok(ev) => self.filter(ev),
                Err(TryRecvError::Empty) => return Ok(()),
                Err(TryRecvError::Disconnected) => return Err(StreamError::Closed),
            }
        }
    }

    /// Credit events go to the credit state, the rest to the user.
    fn filter(&mut self, ev: RawStreamEvent) {
        if let Some(credit) = &mut self.credit {
            match &ev {
                RawStreamEvent::Sideband(sideband) => {
                    let now = credit.now_ms();
                    match credit.tx.on_sideband(sideband, now) {
                        SinkSideband::Credit => return,
                        SinkSideband::DeviceClosed => {
                            // it lost the session (a reboot) or never saw the open: open it again, the answer
                            // tells whether the device still has it
                            credit.reopen = credit.tx.resume(now);
                            return;
                        }
                        SinkSideband::Other => {}
                    }
                }
                RawStreamEvent::Disconnected if credit.tx.session().is_some() => {
                    credit.tx.closed();
                    credit.opening = None;
                    credit.reopen = None;
                    credit.lost = true;
                }
                _ => {}
            }
        }
        self.pending.push_back(ev);
    }
}

fn sideband_or_err(ev: RawStreamEvent) -> Result<Option<StreamSideband>, StreamError> {
    match ev {
        RawStreamEvent::Sideband(sideband) => Ok(Some(sideband)),
        RawStreamEvent::Connected => Ok(None),
        ev => Err(StreamError::UnexpectedEvent(ev)),
    }
}

/// Serialize into a buffer growing up to the 64 KiB a value can take.
fn to_bytes<T: SerializeShrinkWrap>(value: &T) -> Result<Vec<u8>, StreamError> {
    let mut size = 1024;
    loop {
        let mut buf = vec![0u8; size];
        match value.to_ww_bytes(&mut buf) {
            Ok(bytes) => return Ok(bytes.to_vec()),
            Err(e) if size >= 65536 => return Err(e.into()),
            Err(_) => size *= 4,
        }
    }
}

// Intentionally not applicable when byte slices are used (TailBytesOwned does not implement SerializeShrinkWrap)
// impl below for TailBytesOwned is provided instead
impl<T: SerializeShrinkWrap> Sink<T> {
    /// Serialize and send the provided value to a remote device sink.
    /// On a credit sink, waits until the device has room for it.
    pub async fn send(&mut self, value: T) -> Result<(), StreamError> {
        self.send_units(value, 1).await
    }

    /// Serialize and send the provided value to a remote device sink.
    /// On a credit sink, blocks until the device has room for it.
    pub fn send_blocking(&mut self, value: T) -> Result<(), StreamError> {
        self.send_units_blocking(value, 1)
    }

    /// Send the value if it can go right away: false if the device has no room for it yet (credit sinks) or the
    /// command queue is full. For promise-style code polling it.
    pub fn try_send(&mut self, value: &T) -> Result<bool, StreamError> {
        self.try_send_units(value, 1)
    }

    /// [Self::send] of a value that takes `units` positions of a credit sink (at least one): what the device's
    /// `have` and [Self::credit_progress] count, e.g., the samples or bytes the value carries. Ignored by plain
    /// sinks.
    pub async fn send_units(&mut self, value: T, units: u64) -> Result<(), StreamError> {
        let bytes = to_bytes(&value)?;
        self.send_raw(bytes, units).await
    }

    /// Blocking [Self::send_units].
    pub fn send_units_blocking(&mut self, value: T, units: u64) -> Result<(), StreamError> {
        let bytes = to_bytes(&value)?;
        self.send_raw_blocking(bytes, units)
    }

    /// Non-blocking [Self::send_units], see [Self::try_send].
    pub fn try_send_units(&mut self, value: &T, units: u64) -> Result<bool, StreamError> {
        let bytes = to_bytes(&value)?;
        self.try_send_raw(&bytes, units)
    }
}

/// Sinks of owned types that hold a `Vec` or a `String` (`FooOwned`): those implement `SerializeShrinkWrapOwned`
/// rather than `SerializeShrinkWrap`, so they are sent with [Sink::send_owned].
impl<T: wire_weaver::shrink_wrap::SerializeShrinkWrapOwned> Sink<T> {
    /// Serialize and send the provided owned value to a remote device sink.
    /// On a credit sink, waits until the device has room for it.
    pub async fn send_owned(&mut self, value: T) -> Result<(), StreamError> {
        let bytes = value.to_ww_bytes_owned()?;
        self.send_raw(bytes, 1).await
    }

    /// Serialize and send the provided owned value to a remote device sink.
    /// On a credit sink, blocks until the device has room for it.
    pub fn send_owned_blocking(&mut self, value: T) -> Result<(), StreamError> {
        let bytes = value.to_ww_bytes_owned()?;
        self.send_raw_blocking(bytes, 1)
    }

    /// [Self::try_send] for owned values.
    pub fn try_send_owned(&mut self, value: &T) -> Result<bool, StreamError> {
        let bytes = value.to_ww_bytes_owned()?;
        self.try_send_raw(&bytes, 1)
    }
}

/// Units of a byte sink's payload: its bytes, one for an empty one (every item takes a position).
pub(crate) fn byte_units(bytes: &[u8]) -> u64 {
    (bytes.len() as u64).max(1)
}

impl Sink<TailBytesOwned> {
    /// Send bytes as is. On a credit sink, waits until the device has room for them; each byte is a unit.
    pub async fn send_bytes(&mut self, bytes: &[u8]) -> Result<(), StreamError> {
        self.send_raw(bytes.to_vec(), byte_units(bytes)).await
    }

    /// Send bytes as is. On a credit sink, blocks until the device has room for them; each byte is a unit.
    pub fn send_bytes_blocking(&mut self, bytes: &[u8]) -> Result<(), StreamError> {
        self.send_raw_blocking(bytes.to_vec(), byte_units(bytes))
    }

    /// Send bytes if they can go right away: false if the device has no room for them yet (credit sinks) or the
    /// command queue is full.
    pub fn try_send_bytes(&mut self, bytes: &[u8]) -> Result<bool, StreamError> {
        self.try_send_raw(bytes, byte_units(bytes))
    }
}

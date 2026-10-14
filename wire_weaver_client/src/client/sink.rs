use crate::client::credit::{CreditConfig, CreditTx, Due};
use crate::event_loop::commander::TransportCommander;
use crate::event_loop::event_queue::TryRecvError;
use crate::event_loop::rx_dispatcher::StreamUpdateReceiver;
use crate::{Error, RawStreamEvent, StreamError};
use std::collections::VecDeque;
use std::marker::PhantomData;
use std::time::{Duration, Instant};
use wire_weaver::shrink_wrap::SerializeShrinkWrap;
use wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned;
use ww_client_server::{PathKindOwned, StreamSideband};

/// Stream of typed values from host to device.
/// Also holds a sideband channel, in both directions.
///
/// A sink marked `#[credit]` in the API has credit-based flow control: `send()` waits until the device has room for
/// all bytes of the value, `flush()` waits until it accepted everything, see `docs/api/streams.md`. A value bigger
/// than the device's receive buffer fails with [StreamError::ItemTooBig].
pub struct Sink<T> {
    transport_cmd_tx: TransportCommander,
    path_kind: PathKindOwned,
    sideband_rx: StreamUpdateReceiver,
    /// Events received while waiting for credit, for the user
    pending: VecDeque<RawStreamEvent>,
    credit: Option<CreditTx>,
    _phantom: PhantomData<T>,
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
            credit: credit.then(|| CreditTx::new(Instant::now())),
            _phantom: PhantomData,
        }
    }

    /// Send Open command through the sideband channel.
    /// Starts a new session on a credit sink, the device answers with the initial credit.
    pub async fn open(&mut self) -> Result<(), StreamError> {
        if let Some(credit) = &mut self.credit {
            credit.opening(Instant::now());
        }
        self.sideband(StreamSideband::Open).await
    }

    /// Send Open command through the sideband channel.
    /// Starts a new session on a credit sink, the device answers with the initial credit.
    pub fn open_blocking(&mut self) -> Result<(), StreamError> {
        if let Some(credit) = &mut self.credit {
            credit.opening(Instant::now());
        }
        self.sideband_blocking(StreamSideband::Open)
    }

    /// Send Close command through the sideband channel.
    /// On a credit sink, waits until the device accepted everything first, see [Self::flush].
    pub async fn close(&mut self) -> Result<(), StreamError> {
        if self.credit.as_ref().is_some_and(|c| c.is_opened()) {
            self.flush().await?;
        }
        if let Some(credit) = &mut self.credit {
            credit.closed();
        }
        self.sideband(StreamSideband::Close).await
    }

    /// Send Close command through the sideband channel.
    /// On a credit sink, waits until the device accepted everything first, see [Self::flush_blocking].
    pub fn close_blocking(&mut self) -> Result<(), StreamError> {
        if self.credit.as_ref().is_some_and(|c| c.is_opened()) {
            self.flush_blocking()?;
        }
        if let Some(credit) = &mut self.credit {
            credit.closed();
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
            credit.config = config;
        }
    }

    /// Bytes the device accepted and bytes sent in this session of a credit sink (serialized values), e.g., for a
    /// progress bar.
    pub fn credit_progress(&self) -> Option<(u32, u32)> {
        self.credit.as_ref().map(|c| (c.acked(), c.sent()))
    }

    /// Wait until the device accepted everything sent so far (credit sinks), resending what was lost.
    /// Returns right away for other sinks, or when nothing was sent.
    ///
    /// See [Self::flush_blocking] for a blocking variant of this method.
    pub async fn flush(&mut self) -> Result<(), StreamError> {
        if !self.credit.as_ref().is_some_and(|c| c.is_opened()) {
            return Ok(());
        }
        let query = self.credit().query_now(Instant::now());
        self.sideband(query).await?;
        loop {
            self.drain_session()?;
            while let Some(write) = self.credit().next_resend()? {
                self.write(write).await?;
            }
            if self.credit().all_acked() {
                return Ok(());
            }
            self.wait_credit().await?;
        }
    }

    /// Wait until the device accepted everything sent so far (credit sinks), resending what was lost.
    /// Returns right away for other sinks, or when nothing was sent.
    ///
    /// See [Self::flush] for an asynchronous variant of this method.
    pub fn flush_blocking(&mut self) -> Result<(), StreamError> {
        if !self.credit.as_ref().is_some_and(|c| c.is_opened()) {
            return Ok(());
        }
        let query = self.credit().query_now(Instant::now());
        self.sideband_blocking(query)?;
        loop {
            self.drain_session()?;
            while let Some(write) = self.credit().next_resend()? {
                self.write_blocking(write)?;
            }
            if self.credit().all_acked() {
                return Ok(());
            }
            self.wait_credit_blocking()?;
        }
    }

    /// Non-blocking [Self::flush] for promise-style code polling it: true once the device accepted everything.
    pub fn try_flush(&mut self) -> Result<bool, StreamError> {
        if !self.credit.as_ref().is_some_and(|c| c.is_opened()) {
            return Ok(true);
        }
        self.drain_session()?;
        if !self.try_resend()? {
            return Ok(false);
        }
        if self.credit().all_acked() {
            return Ok(true);
        }
        self.try_due()?;
        Ok(false)
    }

    async fn send_raw(&mut self, bytes: Vec<u8>) -> Result<(), StreamError> {
        if self.credit.is_none() {
            self.transport_cmd_tx
                .send_write_request_forget(self.path_kind.clone(), bytes)
                .await?;
            return Ok(());
        }
        if !self.credit().is_opened() {
            self.open().await?;
        }
        loop {
            self.drain_session()?;
            while let Some(write) = self.credit().next_resend()? {
                self.write(write).await?;
            }
            if let Some(write) = self.credit().try_new(&bytes)? {
                return self.write(write).await;
            }
            self.wait_credit().await?;
        }
    }

    fn send_raw_blocking(&mut self, bytes: Vec<u8>) -> Result<(), StreamError> {
        if self.credit.is_none() {
            self.transport_cmd_tx
                .send_write_request_forget_blocking(self.path_kind.clone(), bytes)?;
            return Ok(());
        }
        if !self.credit().is_opened() {
            self.open_blocking()?;
        }
        loop {
            self.drain_session()?;
            while let Some(write) = self.credit().next_resend()? {
                self.write_blocking(write)?;
            }
            if let Some(write) = self.credit().try_new(&bytes)? {
                return self.write_blocking(write);
            }
            self.wait_credit_blocking()?;
        }
    }

    fn try_send_raw(&mut self, bytes: &[u8]) -> Result<bool, StreamError> {
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
        if !self.credit().is_opened() {
            let Some(permit) = self.transport_cmd_tx.try_reserve()? else {
                return Ok(false);
            };
            permit.send(
                self.transport_cmd_tx
                    .sideband_command(self.path_kind.clone(), StreamSideband::Open)?,
            );
            self.credit().opening(Instant::now());
        }
        self.drain_session()?;
        if !self.try_resend()? {
            return Ok(false);
        }
        let Some(permit) = self.transport_cmd_tx.try_reserve()? else {
            return Ok(false);
        };
        match self.credit.as_mut().expect("credit sink").try_new(bytes)? {
            Some(write) => {
                permit.send(
                    self.transport_cmd_tx
                        .write_forget_command(self.path_kind.clone(), write)?,
                );
                Ok(true)
            }
            None => {
                drop(permit);
                self.try_due()?;
                Ok(false)
            }
        }
    }

    /// Resend what the device asked for, without waiting: false if the command queue is full or out of credit.
    fn try_resend(&mut self) -> Result<bool, StreamError> {
        loop {
            let Some(permit) = self.transport_cmd_tx.try_reserve()? else {
                return Ok(false);
            };
            match self.credit.as_mut().expect("credit sink").next_resend()? {
                Some(write) => permit.send(
                    self.transport_cmd_tx
                        .write_forget_command(self.path_kind.clone(), write)?,
                ),
                None => return Ok(true),
            }
        }
    }

    /// Send a query or Open when one is due, without waiting.
    fn try_due(&mut self) -> Result<(), StreamError> {
        let Some(permit) = self.transport_cmd_tx.try_reserve()? else {
            return Ok(());
        };
        let sideband = match self
            .credit
            .as_mut()
            .expect("credit sink")
            .due(Instant::now())
        {
            None => return Ok(()),
            Some(Due::Stalled) => return Err(Error::Timeout.into()),
            Some(Due::Open) => StreamSideband::Open,
            Some(Due::Query(query)) => query,
        };
        permit.send(
            self.transport_cmd_tx
                .sideband_command(self.path_kind.clone(), sideband)?,
        );
        Ok(())
    }

    /// Wait for a credit event, or send a query or Open again when it doesn't come.
    async fn wait_credit(&mut self) -> Result<(), StreamError> {
        let wait = self.credit().wait_time(Instant::now());
        if let Ok(ev) = tokio::time::timeout(wait, self.sideband_rx.recv()).await {
            self.on_event(ev)?;
        }
        let sideband = match self.credit().due(Instant::now()) {
            None => return Ok(()),
            Some(Due::Stalled) => return Err(Error::Timeout.into()),
            Some(Due::Open) => StreamSideband::Open,
            Some(Due::Query(query)) => query,
        };
        self.sideband(sideband).await
    }

    /// Blocking [Self::wait_credit].
    fn wait_credit_blocking(&mut self) -> Result<(), StreamError> {
        let wait = self.credit().wait_time(Instant::now());
        let deadline = Instant::now() + wait;
        loop {
            match self.sideband_rx.try_recv() {
                Ok(ev) => {
                    self.on_event(Some(ev))?;
                    break;
                }
                Err(TryRecvError::Empty) => {
                    let now = Instant::now();
                    if now >= deadline {
                        break;
                    }
                    // tokio's blocking_recv has no timeout variant
                    std::thread::sleep((deadline - now).min(Duration::from_millis(1)));
                }
                Err(TryRecvError::Disconnected) => return Err(StreamError::Closed),
            }
        }
        let sideband = match self.credit().due(Instant::now()) {
            None => return Ok(()),
            Some(Due::Stalled) => return Err(Error::Timeout.into()),
            Some(Due::Open) => StreamSideband::Open,
            Some(Due::Query(query)) => query,
        };
        self.sideband_blocking(sideband)
    }

    fn on_event(&mut self, ev: Option<RawStreamEvent>) -> Result<(), StreamError> {
        let ev = ev.ok_or(StreamError::Closed)?;
        self.filter(ev);
        Ok(())
    }

    /// Take in what arrived, fails if the link went down during a credit sink session: the device lost it, values
    /// sent so far may be lost too.
    fn drain_session(&mut self) -> Result<(), StreamError> {
        self.drain()?;
        if self.credit().is_opened() {
            Ok(())
        } else {
            Err(StreamError::UnexpectedEvent(RawStreamEvent::Disconnected))
        }
    }

    /// Take in everything received so far without waiting.
    fn drain(&mut self) -> Result<(), StreamError> {
        loop {
            match self.sideband_rx.try_recv() {
                Ok(ev) => self.on_event(Some(ev))?,
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
                    if credit.on_sideband(*sideband, Instant::now()) {
                        return;
                    }
                }
                RawStreamEvent::Disconnected => credit.closed(),
                _ => {}
            }
        }
        self.pending.push_back(ev);
    }

    fn credit(&mut self) -> &mut CreditTx {
        self.credit.as_mut().expect("credit sink")
    }

    async fn write(&self, write: Vec<u8>) -> Result<(), StreamError> {
        self.transport_cmd_tx
            .send_write_request_forget(self.path_kind.clone(), write)
            .await?;
        Ok(())
    }

    fn write_blocking(&self, write: Vec<u8>) -> Result<(), StreamError> {
        self.transport_cmd_tx
            .send_write_request_forget_blocking(self.path_kind.clone(), write)?;
        Ok(())
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
        let bytes = to_bytes(&value)?;
        self.send_raw(bytes).await
    }

    /// Serialize and send the provided value to a remote device sink.
    /// On a credit sink, blocks until the device has room for it.
    pub fn send_blocking(&mut self, value: T) -> Result<(), StreamError> {
        let bytes = to_bytes(&value)?;
        self.send_raw_blocking(bytes)
    }

    /// Send the value if it can go right away: false if the device has no room for it yet (credit sinks) or the
    /// command queue is full. For promise-style code polling it.
    pub fn try_send(&mut self, value: &T) -> Result<bool, StreamError> {
        let bytes = to_bytes(&value)?;
        self.try_send_raw(&bytes)
    }
}

/// Sinks of owned types that hold a `Vec` or a `String` (`FooOwned`): those implement `SerializeShrinkWrapOwned`
/// rather than `SerializeShrinkWrap`, so they are sent with [Sink::send_owned].
impl<T: wire_weaver::shrink_wrap::SerializeShrinkWrapOwned> Sink<T> {
    /// Serialize and send the provided owned value to a remote device sink.
    /// On a credit sink, waits until the device has room for it.
    pub async fn send_owned(&mut self, value: T) -> Result<(), StreamError> {
        let bytes = value.to_ww_bytes_owned()?;
        self.send_raw(bytes).await
    }

    /// Serialize and send the provided owned value to a remote device sink.
    /// On a credit sink, blocks until the device has room for it.
    pub fn send_owned_blocking(&mut self, value: T) -> Result<(), StreamError> {
        let bytes = value.to_ww_bytes_owned()?;
        self.send_raw_blocking(bytes)
    }

    /// [Self::try_send] for owned values.
    pub fn try_send_owned(&mut self, value: &T) -> Result<bool, StreamError> {
        let bytes = value.to_ww_bytes_owned()?;
        self.try_send_raw(&bytes)
    }
}

impl Sink<TailBytesOwned> {
    /// Send bytes as is. On a credit sink, waits until the device has room for them.
    pub async fn send_bytes(&mut self, bytes: &[u8]) -> Result<(), StreamError> {
        self.send_raw(bytes.to_vec()).await
    }

    /// Send bytes as is. On a credit sink, blocks until the device has room for them.
    pub fn send_bytes_blocking(&mut self, bytes: &[u8]) -> Result<(), StreamError> {
        self.send_raw_blocking(bytes.to_vec())
    }

    /// Send bytes if they can go right away: false if the device has no room for them yet (credit sinks) or the
    /// command queue is full.
    pub fn try_send_bytes(&mut self, bytes: &[u8]) -> Result<bool, StreamError> {
        self.try_send_raw(bytes)
    }
}

//! Counted and reliable streams in the dynamic client (CLT-23): [DynPositionedStream], items as [ValueOwned].
//! Batched streams (API-41) give one item per entry of a batch, same as the typed [PositionedStream](crate::PositionedStream).

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned;
use ww_self::{ApiBundleOwned, Batching, FieldsOwned, FieldsValueOwned, TypeOwned, ValueOwned};
use ww_si::Prefix;

use crate::client::device_time::DeviceTime;
use crate::client::dynamic::decode_item;
use crate::{
    Batch, BatchItem, Error, PositionedStream, Second, StreamCursor, StreamError, StreamEvent,
    StreamStats,
};

/// What opening a counted or reliable stream in the dynamic client gave, see
/// [DynResource::positioned_stream](crate::DynResource::positioned_stream).
pub enum DynStreamOpen {
    /// Open, with the stream function's result (its `Ok` value for a `Result`, None without a result).
    Open {
        result: Option<ValueOwned>,
        stream: Box<DynPositionedStream>,
    },
    /// The stream function returned this `Err`: nothing was opened.
    Rejected(ValueOwned),
}

/// Counted or reliable stream from a device with items as [ValueOwned], see [PositionedStream]. From
/// [DynResource::positioned_stream](crate::DynResource::positioned_stream).
pub struct DynPositionedStream {
    pub(crate) stream: PositionedStream<TailBytesOwned>,
    pub(crate) ty: TypeOwned,
    pub(crate) is_bytes: bool,
    pub(crate) bundle: Arc<ApiBundleOwned>,
    /// How a batched stream's events decode into items (API-41), `None`: the stream isn't batched, an event is
    /// one item.
    pub(crate) batched: Option<DynBatching>,
    /// Items of the last batch not taken yet.
    pub(crate) pending: VecDeque<StreamEvent<ValueOwned>>,
}

/// The wire item of a batched stream (API-41) and the device time its items unwrap against, for
/// [DynPositionedStream]. Built once when the stream is opened, from
/// [StreamFnRef::wire_item_ty](ww_self::StreamFnRef::wire_item_ty).
pub(crate) struct DynBatching {
    /// `Series<Timed<T>>` or `Series<T>` (untimed).
    wire_item_ty: TypeOwned,
    /// `Timed<T>`'s `t` field type (always `ww_si::Second`), `None` for an untimed batch (no time to unwrap).
    second_ty: Option<TypeOwned>,
    time: DeviceTime,
}

impl DynBatching {
    /// Built from `stream_fn.wire_item_ty()`, `None` when the stream isn't batched.
    pub(crate) fn new(
        batched: Option<Batching>,
        wire_item_ty: TypeOwned,
        bundle: &ApiBundleOwned,
    ) -> Result<Option<Self>, Error> {
        let Some(batched) = batched else {
            return Ok(None);
        };
        let second_ty = match batched {
            Batching::Timed => Some(second_field_ty(&wire_item_ty, bundle)?),
            Batching::Untimed | Batching::Unknown(_) => None,
            // levels of observed properties only, a stream has no level to share a clock with
            Batching::SharedClock => {
                return Err(Error::Other(
                    "batched stream: SharedClock is for observed properties, malformed".into(),
                ));
            }
        };
        Ok(Some(DynBatching {
            wire_item_ty,
            second_ty,
            time: DeviceTime::default(),
        }))
    }

    pub(crate) fn fork(&self) -> Self {
        DynBatching {
            wire_item_ty: self.wire_item_ty.clone(),
            second_ty: self.second_ty.clone(),
            time: DeviceTime::default(),
        }
    }

    /// The device's clock may have restarted (a reconnect): the next time starts the count again.
    pub(crate) fn restart(&mut self) {
        self.time.restart();
    }

    /// The items of one event's bytes, `t` unwrapped (`None` for an untimed batch).
    pub(crate) fn decode(
        &mut self,
        bytes: &[u8],
        bundle: &ApiBundleOwned,
    ) -> Result<Vec<BatchItem<ValueOwned>>, StreamError> {
        let ValueOwned::Vec(items) =
            ValueOwned::des_shrink_wrap_dyn(bytes, &self.wire_item_ty, bundle)
                .map_err(|e| StreamError::Other(Error::Other(format!("batched stream: {e:#}"))))?
        else {
            return Err(StreamError::Other(Error::Other(
                "batched stream: expected a Series value".into(),
            )));
        };
        let Some(second_ty) = &self.second_ty else {
            // untimed: every entry is the item itself, no time to unwrap
            return Ok(items
                .into_iter()
                .map(|value| BatchItem { t: None, value })
                .collect());
        };
        items
            .into_iter()
            .map(|item| {
                let ValueOwned::Struct {
                    fields: FieldsValueOwned::Named(fields),
                } = item
                else {
                    return Err(StreamError::Other(Error::Other(
                        "batched stream: expected a Timed<T> value".into(),
                    )));
                };
                let mut t = None;
                let mut value = None;
                for (name, v) in fields {
                    match name.as_str() {
                        "t" => t = Some(v),
                        "value" => value = Some(v),
                        _ => {}
                    }
                }
                let (t, value) = t.zip(value).ok_or_else(|| {
                    StreamError::Other(Error::Other(
                        "batched stream: Timed<T> value is missing 't' or 'value'".into(),
                    ))
                })?;
                let second = crate::quantity::DynQuantity::from_value(&t, second_ty, bundle)
                    .ok_or_else(|| {
                        StreamError::Other(Error::Other(
                            "batched stream: 't' is not a quantity".into(),
                        ))
                    })?;
                let (prefix, number) = second.si_parts().map_err(|e| {
                    StreamError::Other(Error::Other(format!("batched stream: {e}")))
                })?;
                let t = Second::from_parts(prefix, number);
                Ok(BatchItem {
                    t: Some(self.time.unwrap(t)),
                    value,
                })
            })
            .collect()
    }
}

/// `Timed<T>`'s `t` field type (`ww_si::Second`) out of `wire_item_ty` (`Series<Timed<T>>`).
fn second_field_ty(wire_item_ty: &TypeOwned, bundle: &ApiBundleOwned) -> Result<TypeOwned, Error> {
    let other = |msg: String| Error::Other(format!("batched timed stream: {msg}"));
    let TypeOwned::Series(timed) = wire_item_ty else {
        return Err(other(format!("expected a Series, got {wire_item_ty:?}")));
    };
    let timed = timed
        .get_in_line(bundle)
        .map_err(|e| other(format!("{e:#}")))?;
    let TypeOwned::Struct(timed) = timed else {
        return Err(other(format!("expected Timed<T>, got {timed:?}")));
    };
    let FieldsOwned::Named(fields) = &timed.fields else {
        return Err(other("Timed<T> has no named fields".into()));
    };
    fields
        .iter()
        .find(|f| f.ident.as_deref() == Some("t"))
        .map(|f| f.ty.clone())
        .ok_or_else(|| other("Timed<T> has no 't' field".into()))
}

impl DynPositionedStream {
    /// Stream item type.
    pub fn ty(&self) -> &TypeOwned {
        &self.ty
    }

    /// Whether this stream is batched (API-41): an event carries one batch, [recv_event](Self::recv_event) and the
    /// others still give one [StreamEvent::Item] per entry.
    pub fn is_batched(&self) -> bool {
        self.batched.is_some()
    }

    /// See [PositionedStream::recv_event].
    pub async fn recv_event(&mut self) -> Result<StreamEvent<ValueOwned>, StreamError> {
        loop {
            if let Some(ev) = self.pending.pop_front() {
                return Ok(ev);
            }
            let ev = self.stream.recv_event().await?;
            self.decode(ev)?;
        }
    }

    /// See [PositionedStream::recv_event_blocking].
    pub fn recv_event_blocking(&mut self) -> Result<StreamEvent<ValueOwned>, StreamError> {
        loop {
            if let Some(ev) = self.pending.pop_front() {
                return Ok(ev);
            }
            let ev = self.stream.recv_event_blocking()?;
            self.decode(ev)?;
        }
    }

    /// See [PositionedStream::recv_event_timeout_blocking].
    pub fn recv_event_timeout_blocking(
        &mut self,
        timeout: Duration,
    ) -> Result<StreamEvent<ValueOwned>, StreamError> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(ev) = self.pending.pop_front() {
                return Ok(ev);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Error::Timeout.into());
            }
            let ev = self.stream.recv_event_timeout_blocking(left)?;
            self.decode(ev)?;
        }
    }

    /// See [PositionedStream::try_recv_event].
    pub fn try_recv_event(&mut self) -> Result<Option<StreamEvent<ValueOwned>>, StreamError> {
        loop {
            if let Some(ev) = self.pending.pop_front() {
                return Ok(Some(ev));
            }
            match self.stream.try_recv_event()? {
                Some(ev) => self.decode(ev)?,
                None => return Ok(None),
            }
        }
    }

    /// Next value, see [PositionedStream::recv].
    pub async fn recv(&mut self) -> Result<ValueOwned, StreamError> {
        loop {
            if let StreamEvent::Item { value, .. } = self.recv_event().await? {
                return Ok(value);
            }
        }
    }

    /// Next value, see [PositionedStream::recv_blocking].
    pub fn recv_blocking(&mut self) -> Result<ValueOwned, StreamError> {
        loop {
            if let StreamEvent::Item { value, .. } = self.recv_event_blocking()? {
                return Ok(value);
            }
        }
    }

    /// The rest of the next batch, or the next one whole (API-41): its items, each with its device time, for code
    /// that works on blocks. Gaps, resumes and disconnects are skipped as by [Self::recv]. An item of a stream that
    /// isn't batched is a batch of one.
    pub async fn recv_batch(&mut self) -> Result<Batch<ValueOwned>, StreamError> {
        loop {
            let ev = self.recv_event().await?;
            if let Some(batch) = self.batch_from(ev) {
                return Ok(batch);
            }
        }
    }

    /// Blocking variant of [Self::recv_batch].
    pub fn recv_batch_blocking(&mut self) -> Result<Batch<ValueOwned>, StreamError> {
        loop {
            let ev = self.recv_event_blocking()?;
            if let Some(batch) = self.batch_from(ev) {
                return Ok(batch);
            }
        }
    }

    /// `ev` and the items of its batch still pending, if it is an item.
    fn batch_from(&mut self, ev: StreamEvent<ValueOwned>) -> Option<Batch<ValueOwned>> {
        let StreamEvent::Item {
            pos, t, run, value, ..
        } = ev
        else {
            return None;
        };
        let mut items = vec![BatchItem { t, value }];
        // the pending events are the rest of this item's batch, nothing else
        items.extend(self.pending.drain(..).filter_map(|ev| match ev {
            StreamEvent::Item { t, value, .. } => Some(BatchItem { t, value }),
            _ => None,
        }));
        Some(Batch { pos, run, items })
    }

    /// See [PositionedStream::split].
    pub fn split(self) -> (DynPositionedStream, DynPositionedStream) {
        let (live, backfill) = self.stream.split();
        (
            DynPositionedStream {
                stream: live,
                ty: self.ty.clone(),
                is_bytes: self.is_bytes,
                bundle: self.bundle.clone(),
                batched: self.batched.as_ref().map(DynBatching::fork),
                pending: VecDeque::new(),
            },
            DynPositionedStream {
                stream: backfill,
                ty: self.ty,
                is_bytes: self.is_bytes,
                bundle: self.bundle,
                batched: self.batched.map(|b| b.fork()),
                pending: VecDeque::new(),
            },
        )
    }

    /// See [PositionedStream::cursor].
    pub fn cursor(&self) -> Option<StreamCursor> {
        self.stream.cursor()
    }

    /// See [PositionedStream::session].
    pub fn session(&self) -> Option<u32> {
        self.stream.session()
    }

    /// See [PositionedStream::is_backfilling].
    pub fn is_backfilling(&self) -> bool {
        self.stream.is_backfilling()
    }

    /// See [PositionedStream::ack].
    pub fn ack(&self, pos: u64) {
        self.stream.ack(pos)
    }

    /// See [PositionedStream::stats].
    pub fn stats(&self) -> StreamStats {
        self.stream.stats()
    }

    /// See [PositionedStream::close].
    pub async fn close(&self) -> Result<(), StreamError> {
        self.stream.close().await
    }

    /// See [PositionedStream::close_blocking].
    pub fn close_blocking(&self) -> Result<(), StreamError> {
        self.stream.close_blocking()
    }

    /// `ev`'s events, pushed to `self.pending` (0 or more: a batch event the host already had every item of gives
    /// none, a non-batched one always gives exactly one).
    fn decode(&mut self, ev: StreamEvent<TailBytesOwned>) -> Result<(), StreamError> {
        let push = |pending: &mut VecDeque<_>, ev| pending.push_back(ev);
        match ev {
            StreamEvent::Item {
                pos,
                units,
                t,
                ext,
                run,
                value,
            } => match &mut self.batched {
                None => push(
                    &mut self.pending,
                    StreamEvent::Item {
                        pos,
                        units,
                        t,
                        ext,
                        run,
                        value: decode_item(self.is_bytes, &self.ty, &self.bundle, value)?,
                    },
                ),
                Some(batching) => {
                    batching.time.on_head(ext.as_ref());
                    let items = batching.decode(&value, &self.bundle)?;
                    if items.len() as u64 != units {
                        tracing::warn!(
                            "batched stream: an event of {units} units has {} items",
                            items.len()
                        );
                    }
                    // `units` already excludes what the host had (the raw event was reported as if unbatched,
                    // see Receiver::on_item's `skip`); the leading items of the full batch are that difference
                    let skip = items.len().saturating_sub(units as usize);
                    let mut ext = ext;
                    for (i, item) in items.into_iter().enumerate().skip(skip) {
                        push(
                            &mut self.pending,
                            StreamEvent::Item {
                                pos: pos + (i - skip) as u64,
                                units: 1,
                                t: item.t,
                                ext: ext.take(),
                                run,
                                value: item.value,
                            },
                        );
                    }
                }
            },
            StreamEvent::Gap {
                run,
                at,
                units,
                cause,
            } => push(
                &mut self.pending,
                StreamEvent::Gap {
                    run,
                    at,
                    units,
                    cause,
                },
            ),
            StreamEvent::Resumed {
                cursor,
                session,
                ended,
            } => push(
                &mut self.pending,
                StreamEvent::Resumed {
                    cursor,
                    session,
                    ended,
                },
            ),
            StreamEvent::BackfillDone => push(&mut self.pending, StreamEvent::BackfillDone),
            StreamEvent::Disconnected => {
                if let Some(batching) = &mut self.batched {
                    batching.time.restart();
                }
                push(&mut self.pending, StreamEvent::Disconnected)
            }
            StreamEvent::Closed => push(&mut self.pending, StreamEvent::Closed),
        }
        Ok(())
    }
}

/// A device time in seconds, e.g. `t` of a [StreamEvent::Item].
pub fn seconds(t: &Second) -> f64 {
    let value = t.value.to_f64();
    let exp = match t.prefix {
        Prefix::Unit => 0,
        Prefix::Milli => -3,
        Prefix::Micro => -6,
        Prefix::Nano => -9,
        Prefix::Pico => -12,
        Prefix::Kilo => 3,
        Prefix::Mega => 6,
        Prefix::Giga => 9,
        Prefix::Deca => 1,
        Prefix::Hecto => 2,
        Prefix::Tera => 12,
        Prefix::Peta => 15,
        Prefix::Exa => 18,
        Prefix::Zetta => 21,
        Prefix::Yotta => 24,
        Prefix::Ronna => 27,
        Prefix::Quetta => 30,
        Prefix::Deci => -1,
        Prefix::Centi => -2,
        Prefix::Femto => -15,
        Prefix::Atto => -18,
        Prefix::Zepto => -21,
        Prefix::Yocto => -24,
        Prefix::Ronto => -27,
        Prefix::Quecto => -30,
    };
    value * 10f64.powi(exp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ww_numeric::NumericValue;

    #[test]
    fn seconds_of_device_times() {
        let t = Second::from_parts(Prefix::Micro, NumericValue::U32(4_000_000_123));
        assert!((seconds(&t) - 4000.000123).abs() < 1e-9);
        let t = Second::from_parts(Prefix::Milli, NumericValue::U64(1500));
        assert_eq!(seconds(&t), 1.5);
    }
}

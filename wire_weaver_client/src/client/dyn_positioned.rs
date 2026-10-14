//! Counted and reliable streams in the dynamic client (CLT-23): [DynPositionedStream], items as [ValueOwned].

use std::sync::Arc;

use wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned;
use ww_self::{ApiBundleOwned, TypeOwned, ValueOwned};
use ww_si::Prefix;

use crate::client::dynamic::decode_item;
use crate::{PositionedStream, Second, StreamCursor, StreamError, StreamEvent, StreamStats};

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
}

impl DynPositionedStream {
    /// Stream item type.
    pub fn ty(&self) -> &TypeOwned {
        &self.ty
    }

    /// See [PositionedStream::recv_event].
    pub async fn recv_event(&mut self) -> Result<StreamEvent<ValueOwned>, StreamError> {
        let ev = self.stream.recv_event().await?;
        self.decode(ev)
    }

    /// See [PositionedStream::recv_event_blocking].
    pub fn recv_event_blocking(&mut self) -> Result<StreamEvent<ValueOwned>, StreamError> {
        let ev = self.stream.recv_event_blocking()?;
        self.decode(ev)
    }

    /// See [PositionedStream::recv_event_timeout_blocking].
    pub fn recv_event_timeout_blocking(
        &mut self,
        timeout: std::time::Duration,
    ) -> Result<StreamEvent<ValueOwned>, StreamError> {
        let ev = self.stream.recv_event_timeout_blocking(timeout)?;
        self.decode(ev)
    }

    /// See [PositionedStream::try_recv_event].
    pub fn try_recv_event(&mut self) -> Result<Option<StreamEvent<ValueOwned>>, StreamError> {
        match self.stream.try_recv_event()? {
            Some(ev) => self.decode(ev).map(Some),
            None => Ok(None),
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

    /// See [PositionedStream::split].
    pub fn split(self) -> (DynPositionedStream, DynPositionedStream) {
        let (live, backfill) = self.stream.split();
        (
            DynPositionedStream {
                stream: live,
                ty: self.ty.clone(),
                is_bytes: self.is_bytes,
                bundle: self.bundle.clone(),
            },
            DynPositionedStream {
                stream: backfill,
                ty: self.ty,
                is_bytes: self.is_bytes,
                bundle: self.bundle,
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

    fn decode(
        &self,
        ev: StreamEvent<TailBytesOwned>,
    ) -> Result<StreamEvent<ValueOwned>, StreamError> {
        Ok(match ev {
            StreamEvent::Item {
                pos,
                units,
                t,
                ext,
                run,
                value,
            } => StreamEvent::Item {
                pos,
                units,
                t,
                ext,
                run,
                value: decode_item(self.is_bytes, &self.ty, &self.bundle, value)?,
            },
            StreamEvent::Gap {
                run,
                at,
                units,
                cause,
            } => StreamEvent::Gap {
                run,
                at,
                units,
                cause,
            },
            StreamEvent::Resumed {
                cursor,
                session,
                ended,
            } => StreamEvent::Resumed {
                cursor,
                session,
                ended,
            },
            StreamEvent::BackfillDone => StreamEvent::BackfillDone,
            StreamEvent::Disconnected => StreamEvent::Disconnected,
            StreamEvent::Closed => StreamEvent::Closed,
        })
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
        let t = Second {
            prefix: Prefix::Micro,
            value: NumericValue::U32(4_000_000_123),
        };
        assert!((seconds(&t) - 4000.000123).abs() < 1e-9);
        let t = Second {
            prefix: Prefix::Milli,
            value: NumericValue::U64(1500),
        };
        assert_eq!(seconds(&t), 1.5);
    }
}

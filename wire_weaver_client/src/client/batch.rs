//! Batched streams on the host (API-41): the device sends one `Series` per event, the typed handles
//! ([PositionedStream](crate::PositionedStream), [Stream](crate::Stream)) give one item per entry, each with its own
//! device time, or whole batches ([Batch]).

use wire_weaver::shrink_wrap::series::{SeriesColumns, SeriesOwned};
use wire_weaver::shrink_wrap::{DeserializeShrinkWrapOwned, Error as ShrinkWrapError};
use ww_si::ww_numeric::NumericValue;
use ww_si::{Prefix, Second, Timed};
use ww_stream::Run;

/// Decodes one event's bytes of a batched stream into its items, see [unpack_timed] and [unpack_untimed]. Generated
/// client code gives it to the stream handle when the stream is batched.
pub type Unpack<T> = fn(&[u8]) -> Result<Vec<BatchItem<T>>, ShrinkWrapError>;

/// One item of a batch.
#[derive(Debug, Clone, PartialEq)]
pub struct BatchItem<T> {
    /// When the device took it, on its clock (unwrapped, see [PositionedStream](crate::PositionedStream)); `None`
    /// for an untimed batched stream.
    pub t: Option<Second>,
    pub value: T,
}

/// A whole batch, see [PositionedStream::recv_batch](crate::PositionedStream::recv_batch).
#[derive(Debug, Clone, PartialEq)]
pub struct Batch<T> {
    /// Position of the first item (each item is one unit).
    pub pos: u64,
    pub run: Run,
    pub items: Vec<BatchItem<T>>,
}

/// [Unpack] of a timed batched stream: `Series<Timed<T>>`.
pub fn unpack_timed<T>(bytes: &[u8]) -> Result<Vec<BatchItem<T>>, ShrinkWrapError>
where
    Timed<T>: for<'i> SeriesColumns<'i>,
{
    Ok(SeriesOwned::<Timed<T>>::from_ww_bytes_owned(bytes)?
        .into_inner()
        .into_iter()
        .map(|item| BatchItem {
            t: Some(item.t),
            value: item.value,
        })
        .collect())
}

/// [Unpack] of an untimed batched stream: `Series<T>`.
pub fn unpack_untimed<T>(bytes: &[u8]) -> Result<Vec<BatchItem<T>>, ShrinkWrapError>
where
    T: for<'i> SeriesColumns<'i>,
{
    Ok(SeriesOwned::<T>::from_ww_bytes_owned(bytes)?
        .into_inner()
        .into_iter()
        .map(|value| BatchItem { t: None, value })
        .collect())
}

/// How a stream handle unpacks the events of a batched stream, and the device time as unwrapped so far. `None`
/// unpack: the stream isn't batched, an event is one item.
pub(crate) struct Unpacker<T> {
    unpack: Option<Unpack<T>>,
    last_t: Option<u64>,
}

impl<T> Unpacker<T> {
    pub(crate) fn new(unpack: Option<Unpack<T>>) -> Self {
        Unpacker {
            unpack,
            last_t: None,
        }
    }

    /// For another handle of the same stream (a half of a split), with its own times.
    pub(crate) fn fork(&self) -> Self {
        Self::new(self.unpack)
    }

    pub(crate) fn is_batched(&self) -> bool {
        self.unpack.is_some()
    }

    /// The items of one event of a batched stream, with times unwrapped.
    pub(crate) fn unpack(&mut self, bytes: &[u8]) -> Result<Vec<BatchItem<T>>, ShrinkWrapError> {
        let unpack = self.unpack.expect("a batched stream");
        let mut items = unpack(bytes)?;
        for item in &mut items {
            item.t = item.t.map(|t| unwrap_t(&mut self.last_t, t));
        }
        Ok(items)
    }
}

/// A device time of `Second { Micro, U32 }`, which wraps every 71.6 minutes, as microseconds since the device's
/// clock started counting: `Second { Micro, U64 }`, given the previous one in `last`. Other times stay as they are.
pub(crate) fn unwrap_t(last: &mut Option<u64>, t: Second) -> Second {
    let (Prefix::Micro, NumericValue::U32(us)) = (&t.prefix, &t.value) else {
        return t;
    };
    let us = *us as u64;
    let unwrapped = match *last {
        None => us,
        Some(prev) => {
            let mut at = (prev & !0xffff_ffff) | us;
            // wrapped since: more than half the range back
            if at + (1 << 31) < prev {
                at += 1 << 32;
            }
            at
        }
    };
    *last = Some(unwrapped);
    Second::from_parts(Prefix::Micro, NumericValue::U64(unwrapped))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_time_unwrapped() {
        let us32 = |v: u32| Second::from_parts(Prefix::Micro, NumericValue::U32(v));
        let us = |t: &Second| match t.value {
            NumericValue::U64(v) => v,
            _ => panic!("{t:?}"),
        };
        let mut last = None;
        assert_eq!(us(&unwrap_t(&mut last, us32(5))), 5);
        assert_eq!(
            us(&unwrap_t(&mut last, us32(u32::MAX - 10))),
            u32::MAX as u64 - 10
        );
        // wrapped
        assert_eq!(us(&unwrap_t(&mut last, us32(20))), (1 << 32) + 20);
        assert_eq!(us(&unwrap_t(&mut last, us32(1_000))), (1 << 32) + 1_000);
        // a little back (not expected, but not a wrap)
        assert_eq!(us(&unwrap_t(&mut last, us32(900))), (1 << 32) + 900);
        // other times as they are
        let ms = Second::from_parts(Prefix::Milli, NumericValue::U32(7));
        assert_eq!(unwrap_t(&mut last, ms), ms);
    }
}

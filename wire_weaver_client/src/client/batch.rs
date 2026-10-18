//! Batched streams on the host (API-41): the device sends one `Series` per event, the typed handles
//! ([PositionedStream](crate::PositionedStream), [Stream](crate::Stream)) give one item per entry, each with its own
//! device time, or whole batches ([Batch]).

use crate::client::device_time::DeviceTime;
use wire_weaver::shrink_wrap::series::{SeriesColumns, SeriesOwned};
use wire_weaver::shrink_wrap::{DeserializeShrinkWrapOwned, Error as ShrinkWrapError};
use ww_si::{Second, Timed};
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

/// How a stream handle unpacks the events of a batched stream, and the device time as unwrapped so far for it.
/// `None` unpack: the stream isn't batched, an event is one item.
pub(crate) struct Unpacker<T> {
    unpack: Option<Unpack<T>>,
    /// Every device time this handle gives out: items of a batch and event heads
    pub(crate) time: DeviceTime,
}

impl<T> Unpacker<T> {
    pub(crate) fn new(unpack: Option<Unpack<T>>) -> Self {
        Unpacker {
            unpack,
            time: DeviceTime::default(),
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
            item.t = item.t.map(|t| self.time.unwrap(t));
        }
        Ok(items)
    }
}

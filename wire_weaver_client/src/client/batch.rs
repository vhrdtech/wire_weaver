//! Batched streams on the host (API-41): the device sends one `Series` per event, the typed handles
//! ([PositionedStream](crate::PositionedStream), [Stream](crate::Stream)) give one item per entry, each with its own
//! device time, or whole batches ([Batch]).

use crate::client::device_time::DeviceTime;
use tracing::warn;
use wire_weaver::shrink_wrap::series::{SeriesColumns, SeriesOwned};
use wire_weaver::shrink_wrap::{DeserializeShrinkWrapOwned, Error as ShrinkWrapError};
use ww_si::exact_units::units;
use ww_si::{Fixed, FixedScale, Linear, Origin, Point, Second, TimePoint, TimeUnwrapper, Timed};
use ww_stream::Run;

/// One item of a batch, taken at `t` (a [Second] of the event head, or for a timed batched stream the item's own
/// time as a full point, `DeviceMicros<u64>` by default).
#[derive(Debug, Clone, PartialEq)]
pub struct BatchItem<T, Tf = Second> {
    /// When the device took it, on its clock (unwrapped, see [PositionedStream](crate::PositionedStream)); `None`
    /// for an untimed batched stream.
    pub t: Option<Tf>,
    pub value: T,
}

/// A whole batch, see [PositionedStream::recv_batch](crate::PositionedStream::recv_batch).
#[derive(Debug, Clone, PartialEq)]
pub struct Batch<T, Tf = Second> {
    /// Position of the first item (each item is one unit).
    pub pos: u64,
    pub run: Run,
    pub items: Vec<BatchItem<T, Tf>>,
}

/// The time a stream handle gives with its items (CLT-70): [Second] for a stream that isn't batched (the event
/// head's time) or is batched untimed, the full [TimePoint] (`<Tm as TimePoint>::Full`) of a timed batched stream.
pub trait ItemTime: Clone + core::fmt::Debug + Send + 'static {
    /// The event head's time as an item's time: `None` where the stream's items carry their own (a timed batched
    /// stream's events have no head time).
    fn from_head(t: Second) -> Option<Self>;
}

impl ItemTime for Second {
    fn from_head(t: Second) -> Option<Self> {
        Some(t)
    }
}

macro_rules! item_time_point {
    ($($t:ty)*) => {$(
        impl<O, P> ItemTime for Point<O, Fixed<units::Second, P, $t>>
        where
            O: Origin<Unit = units::Second, Modulus = Linear> + core::fmt::Debug + Send,
            P: FixedScale + core::fmt::Debug + Send + 'static,
        {
            fn from_head(_: Second) -> Option<Self> {
                None
            }
        }
    )*};
}

item_time_point!(u64 i64);

/// Decodes the events of a batched stream into items for one stream handle (API-41), keeping what it needs from one
/// event to the next (the device time unwrapped so far): [unpack_timed] and [unpack_untimed]. Generated client code
/// gives one to the stream handle when the stream is batched.
pub trait Unpack<T, Tf>: Send + Sync {
    /// The items of one event, with times unwrapped.
    fn unpack(&mut self, bytes: &[u8]) -> Result<Vec<BatchItem<T, Tf>>, ShrinkWrapError>;

    /// The device's clock may have restarted (a reconnect, a new clock epoch): the next time starts the count again.
    fn restart(&mut self);

    /// A new one for another handle of the same stream (a half of a split), with its own times.
    fn fork(&self) -> Box<dyn Unpack<T, Tf>>;
}

/// [Unpack] of a timed batched stream, `Series<Timed<T, Tm>>`: each item with its time unwrapped into the full point
/// (`DeviceMicros<u32>` on the wire, `DeviceMicros<u64>` here).
pub fn unpack_timed<T, Tm>() -> Box<dyn Unpack<T, Tm::Full>>
where
    T: Send + 'static,
    Tm: TimePoint + Send + 'static,
    Timed<T, Tm>: for<'i> SeriesColumns<'i>,
{
    Box::new(Timeds::<T, Tm> {
        unwrapper: TimeUnwrapper::default(),
        _t: core::marker::PhantomData,
    })
}

/// [Unpack] of an untimed batched stream, `Series<T>`.
pub fn unpack_untimed<T>() -> Box<dyn Unpack<T, Second>>
where
    T: Send + 'static,
    T: for<'i> SeriesColumns<'i>,
{
    Box::new(Untimed::<T>(core::marker::PhantomData))
}

struct Timeds<T, Tm> {
    unwrapper: TimeUnwrapper<Tm>,
    _t: core::marker::PhantomData<fn() -> T>,
}

impl<T, Tm> Unpack<T, Tm::Full> for Timeds<T, Tm>
where
    T: Send + 'static,
    Tm: TimePoint + Send + 'static,
    Timed<T, Tm>: for<'i> SeriesColumns<'i>,
{
    fn unpack(&mut self, bytes: &[u8]) -> Result<Vec<BatchItem<T, Tm::Full>>, ShrinkWrapError> {
        Ok(SeriesOwned::<Timed<T, Tm>>::from_ww_bytes_owned(bytes)?
            .into_inner()
            .into_iter()
            .map(|item| BatchItem {
                t: match self.unwrapper.unwrap(item.t) {
                    Ok(t) => Some(t),
                    Err(e) => {
                        warn!("device time {}: {e}, left out", item.t.raw_int());
                        None
                    }
                },
                value: item.value,
            })
            .collect())
    }

    fn restart(&mut self) {
        self.unwrapper.restart();
    }

    fn fork(&self) -> Box<dyn Unpack<T, Tm::Full>> {
        unpack_timed::<T, Tm>()
    }
}

struct Untimed<T>(core::marker::PhantomData<fn() -> T>);

impl<T> Unpack<T, Second> for Untimed<T>
where
    T: Send + 'static,
    T: for<'i> SeriesColumns<'i>,
{
    fn unpack(&mut self, bytes: &[u8]) -> Result<Vec<BatchItem<T, Second>>, ShrinkWrapError> {
        Ok(SeriesOwned::<T>::from_ww_bytes_owned(bytes)?
            .into_inner()
            .into_iter()
            .map(|value| BatchItem { t: None, value })
            .collect())
    }

    fn restart(&mut self) {}

    fn fork(&self) -> Box<dyn Unpack<T, Second>> {
        unpack_untimed::<T>()
    }
}

/// How a stream handle unpacks the events of a batched stream, and the device time as unwrapped so far for it.
/// `None` unpack: the stream isn't batched, an event is one item.
pub(crate) struct Unpacker<T, Tf> {
    unpack: Option<Box<dyn Unpack<T, Tf>>>,
    /// The event heads' device times
    time: DeviceTime,
}

impl<T, Tf: ItemTime> Unpacker<T, Tf> {
    pub(crate) fn new(unpack: Option<Box<dyn Unpack<T, Tf>>>) -> Self {
        Unpacker {
            unpack,
            time: DeviceTime::default(),
        }
    }

    /// For another handle of the same stream (a half of a split), with its own times.
    pub(crate) fn fork(&self) -> Self {
        Self::new(self.unpack.as_ref().map(|u| u.fork()))
    }

    pub(crate) fn is_batched(&self) -> bool {
        self.unpack.is_some()
    }

    /// The device may have rebooted (a reconnect): every time starts the count again.
    pub(crate) fn restart(&mut self) {
        self.time.restart();
        if let Some(unpack) = &mut self.unpack {
            unpack.restart();
        }
    }

    /// Before the times of a stream event: a new clock epoch in its head restarts the count.
    pub(crate) fn on_head(&mut self, ext: Option<&ww_stream::HeadExt>) {
        if ext.is_some_and(|ext| ext.clock_epoch.is_some()) {
            self.restart();
        }
    }

    /// An event head's time, unwrapped, as the items' time.
    pub(crate) fn head_time(&mut self, t: Option<Second>) -> Option<Tf> {
        t.map(|t| self.time.unwrap(t)).and_then(Tf::from_head)
    }

    /// The items of one event of a batched stream, with times unwrapped.
    pub(crate) fn unpack(
        &mut self,
        bytes: &[u8],
    ) -> Result<Vec<BatchItem<T, Tf>>, ShrinkWrapError> {
        self.unpack
            .as_mut()
            .expect("a batched stream")
            .unpack(bytes)
    }
}

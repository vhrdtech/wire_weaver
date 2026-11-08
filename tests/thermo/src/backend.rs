//! What the application sees of a device, whatever its line: one implementation per line (`v1.rs` to `v3.rs`), chosen
//! once at connect time. A trait object rather than an `enum { V1, V2, V3 }`: a new line is a new module and one arm in
//! `Thermo::from_cmd`, no method of [crate::Thermo] grows an arm per line.

use crate::error::Result;
use crate::model::{Command, Gain, Mode, Offset, Rate, Sample, Uptime};
use std::future::Future;
use std::pin::Pin;
use wire_weaver::ww_version::WireVersion;
use wire_weaver_client::Commander;
use wire_weaver_client::observe::{LevelUpdate, Observer};

/// `async fn` isn't allowed in a trait object yet: every operation returns its future boxed.
pub(crate) type Fut<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

pub(crate) trait Backend: Send + Sync {
    fn cmd(&self) -> &Commander;
    /// The device's wire version, from link setup.
    fn wire(&self) -> WireVersion;
    fn configure(&self, rate: Rate, mode: Mode) -> Fut<'_, ()>;
    fn read_once(&self) -> Fut<'_, Sample>;
    fn mode(&self) -> Fut<'_, Mode>;
    fn set_mode(&self, mode: Mode) -> Fut<'_, ()>;
    fn gain(&self) -> Fut<'_, Gain>;
    fn set_gain(&self, gain: Gain) -> Fut<'_, ()>;
    fn observe_gain(&self) -> Fut<'_, Box<dyn Updates<Gain>>>;
    fn samples(&self) -> Fut<'_, Box<dyn Updates<Sample>>>;
    fn commands(&self) -> Fut<'_, Box<dyn Commands>>;
    fn channel_enabled(&self, channel: u32) -> Fut<'_, bool>;
    fn set_channel_enabled(&self, channel: u32, enabled: bool) -> Fut<'_, ()>;
    fn channel_last(&self, channel: u32) -> Fut<'_, Sample>;
    fn calibrate(&self, offset: Offset) -> Fut<'_, ()>;
    fn uptime(&self) -> Fut<'_, Uptime>;
    fn self_test(&self) -> Fut<'_, ()>;
}

/// Items of a stream or an observed property, converted to the model.
pub trait Updates<T>: Send {
    fn recv(&mut self) -> Fut<'_, T>;
}

/// The command sink, taking the model's commands.
pub trait Commands: Send {
    fn send(&mut self, command: Command) -> Fut<'_, ()>;
}

/// A line's stream of `A`, its items converted by `f`.
pub(crate) struct StreamOf<A, T> {
    pub(crate) stream: wire_weaver_client::Stream<A>,
    pub(crate) f: fn(A) -> Result<T>,
}

impl<A, T> Updates<T> for StreamOf<A, T>
where
    A: wire_weaver::shrink_wrap::DeserializeShrinkWrapOwned + Send + 'static,
    T: Send + 'static,
{
    fn recv(&mut self) -> Fut<'_, T> {
        Box::pin(async move { (self.f)(self.stream.recv().await?) })
    }
}

/// A line's observer of a property of type `A`, its values converted by `f`.
pub(crate) struct ObserverOf<U: LevelUpdate, A, T> {
    pub(crate) observer: Observer<U, A>,
    pub(crate) f: fn(A) -> Result<T>,
}

impl<U, A, T> Updates<T> for ObserverOf<U, A, T>
where
    U: LevelUpdate + Send + 'static,
    U::Latest: Send,
    A: Send + 'static,
    T: Send + 'static,
{
    fn recv(&mut self) -> Fut<'_, T> {
        Box::pin(async move { (self.f)(self.observer.recv().await?.value) })
    }
}

/// A line's sink of `A`, the model's commands converted by `f`.
pub(crate) struct SinkOf<A> {
    pub(crate) sink: wire_weaver_client::Sink<A>,
    pub(crate) f: fn(Command) -> Result<A>,
}

impl<A> Commands for SinkOf<A>
where
    A: wire_weaver::shrink_wrap::SerializeShrinkWrapOwned + Send + 'static,
{
    fn send(&mut self, command: Command) -> Fut<'_, ()> {
        Box::pin(async move {
            let item = (self.f)(command)?;
            Ok(self.sink.send_owned(item).await?)
        })
    }
}

/// A line's stream of `A`, each item split into several `T` by `f` (line 3 sends batches of samples).
pub(crate) struct SplitStreamOf<A, T> {
    pub(crate) stream: wire_weaver_client::Stream<A>,
    pub(crate) f: fn(A) -> Result<Vec<T>>,
    pub(crate) pending: std::collections::VecDeque<T>,
}

impl<A, T> SplitStreamOf<A, T> {
    pub(crate) fn new(stream: wire_weaver_client::Stream<A>, f: fn(A) -> Result<Vec<T>>) -> Self {
        Self {
            stream,
            f,
            pending: Default::default(),
        }
    }
}

impl<A, T> Updates<T> for SplitStreamOf<A, T>
where
    A: wire_weaver::shrink_wrap::DeserializeShrinkWrapOwned + Send + 'static,
    T: Send + 'static,
{
    fn recv(&mut self) -> Fut<'_, T> {
        Box::pin(async move {
            loop {
                if let Some(item) = self.pending.pop_front() {
                    return Ok(item);
                }
                self.pending = (self.f)(self.stream.recv().await?)?.into();
            }
        })
    }
}

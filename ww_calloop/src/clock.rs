//! [ww_device::Clock] on the loop's timers.

use std::future::{Future, pending};
use std::time::{Duration, Instant};

use calloop::LoopHandle;
use calloop::timer::TimeoutFuture;
use futures_util::future::Either;

/// A [ww_device::Clock] whose waits are timers of a calloop loop. Clones share their start, so they tell the same
/// time.
pub struct LoopClock<'l, D> {
    handle: LoopHandle<'l, D>,
    start: Instant,
}

impl<'l, D> LoopClock<'l, D> {
    /// A clock that starts now, its timers on the loop of `handle`.
    pub fn new(handle: LoopHandle<'l, D>) -> Self {
        LoopClock {
            handle,
            start: Instant::now(),
        }
    }
}

impl<D> Clone for LoopClock<'_, D> {
    fn clone(&self) -> Self {
        LoopClock {
            handle: self.handle.clone(),
            start: self.start,
        }
    }
}

impl<D> ww_device::Clock for LoopClock<'_, D> {
    fn now(&self) -> ww_device::Instant {
        // u64 microseconds last 584,000 years
        let micros = u64::try_from(self.start.elapsed().as_micros()).unwrap_or(u64::MAX);
        ww_device::Instant::from_micros(micros)
    }

    /// An instant too far to name as a deadline never comes.
    fn wait_until(&self, at: ww_device::Instant) -> impl Future<Output = ()> {
        match self
            .start
            .checked_add(Duration::from_micros(at.as_micros()))
        {
            Some(deadline) => Either::Left(TimeoutFuture::from_deadline(&self.handle, deadline)),
            None => Either::Right(pending()),
        }
    }
}

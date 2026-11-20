//! What a connection's backend is to the server.

use std::future::{Future, pending};

use wire_weaver::{EventWriter, MessageSink, WireWeaverAsyncApiBackend};
use ww_device::Instant;

/// The backend of one connection: the generated server, and what it does on its own between requests.
///
/// A backend that only answers requests needs nothing but `impl Backend for MyBackend {}`. One that sends without
/// being asked (stream items, observed properties) says when with [wait](Self::wait) and
/// [next_wake](Self::next_wake), and sends in [poll](Self::poll):
///
/// ```ignore
/// impl Backend for MyBackend {
///     async fn wait(&mut self) {
///         // something of the service changed: an item for this client's stream
///         self.item = self.news.recv().await;
///     }
///
///     fn next_wake(&mut self, now: Instant) -> Option<Instant> {
///         // an observed property is due
///         self.observe.next_wake(now.as_micros()).map(Instant::from_micros)
///     }
///
///     async fn poll(&mut self, out: &mut EventWriter<'_, impl MessageSink>, _medium: (), now: Instant) {
///         if let Some(Ok(item)) = self.item.take() {
///             let _ = api_impl::stream_data_ser().news_send(&item, out).await;
///         }
///         let mut buf = std::mem::take(&mut self.buf);
///         let _ = self.poll_properties(out, (), now.as_micros(), &mut buf).await;
///         self.buf = buf;
///     }
/// }
/// ```
pub trait Backend: WireWeaverAsyncApiBackend {
    /// Completes when the backend has something to do on its own. The connection waits on it while no request
    /// runs and drops the future when a request comes, so it must be cancel-safe: what it took in before it was
    /// dropped stays in `self`. Never completes by default.
    fn wait(&mut self) -> impl Future<Output = ()> {
        pending()
    }

    /// When the backend wants [poll](Self::poll) next, without a request and without [wait](Self::wait)
    /// completing: the time an observed property is due. `None` by default.
    fn next_wake(&mut self, now: Instant) -> Option<Instant> {
        let _ = now;
        None
    }

    /// Send what is due. Called after every request, when [wait](Self::wait) completed and at
    /// [next_wake](Self::next_wake); it runs to its end before the next request starts. Does nothing by default.
    fn poll(
        &mut self,
        out: &mut EventWriter<'_, impl MessageSink>,
        medium: Self::Medium,
        now: Instant,
    ) -> impl Future<Output = ()> {
        let _ = (out, medium, now);
        async {}
    }
}

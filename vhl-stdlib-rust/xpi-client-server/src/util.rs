use futures::Stream;
use std::time::Duration;

pub fn tick_stream(period: Duration) -> impl Stream<Item=()> {
    futures::stream::unfold(period, move |p| async move {
        tokio::time::sleep(period).await;
        Some(((), p))
    })
}

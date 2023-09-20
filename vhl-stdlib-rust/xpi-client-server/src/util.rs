use futures::Stream;
use std::cell::RefCell;
use std::time::Duration;

pub fn tick_stream(period: Duration) -> impl Stream<Item = ()> {
    futures::stream::unfold(period, move |p| async move {
        tokio::time::sleep(period).await;
        Some(((), p))
    })
}

pub struct IteratorAdapter<I>(pub RefCell<I>);

impl<I> IteratorAdapter<I> {
    pub fn new(iterator: I) -> Self {
        Self(RefCell::new(iterator))
    }
}

impl<I> serde::Serialize for IteratorAdapter<I>
where
    I: Iterator,
    I::Item: serde::Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.collect_seq(self.0.borrow_mut().by_ref())
    }
}

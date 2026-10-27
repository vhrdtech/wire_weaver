//! A timer that wakes on time to a few tens of microseconds, for the frame accumulation window (CLT-50): tokio's
//! timer counts whole milliseconds and rounds a deadline up to the next one, so a 1 ms window cost 1-2 ms more than
//! asked. One thread for the whole process keeps the deadlines that are near and wakes their waiters; deadlines
//! further away wait on tokio's timer first.

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::OnceLock;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use tokio::sync::oneshot;

/// Deadlines closer than this are left to the fine timer thread, further ones first wait on tokio's timer for the
/// rest: its rounding (up to 1 ms late) is then still before the deadline.
const FINE_SPAN: Duration = Duration::from_millis(2);

/// Sleep until `deadline` on tokio's clock, waking within a few tens of microseconds of it on a running clock.
///
/// On a paused clock (`start_paused` tests) the fine timer wakes on the wall clock, while tokio's clock stands
/// still: the sleep then ends when tokio's own timer does, as it moves the paused clock forward.
pub(crate) async fn sleep_until(deadline: tokio::time::Instant) {
    let fine = async {
        if let Some(coarse) = deadline.checked_sub(FINE_SPAN)
            && coarse > tokio::time::Instant::now()
        {
            tokio::time::sleep_until(coarse).await;
        }
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        if !left.is_zero() {
            wait(left).await;
        }
        if tokio::time::Instant::now() < deadline {
            // the clock is paused: tokio's timer ends the sleep
            std::future::pending::<()>().await;
        }
    };
    tokio::select! {
        _ = fine => {}
        _ = tokio::time::sleep_until(deadline) => {}
    }
}

/// Waits `d` on the wall clock, on the fine timer thread.
async fn wait(d: Duration) {
    let (tx, rx) = oneshot::channel();
    if thread().send((Instant::now() + d, tx)).is_err() {
        // no timer thread: tokio's timer ends the sleep
        return std::future::pending().await;
    }
    _ = rx.await;
}

type Entry = (Instant, oneshot::Sender<()>);

fn thread() -> &'static mpsc::Sender<Entry> {
    static TX: OnceLock<mpsc::Sender<Entry>> = OnceLock::new();
    TX.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<Entry>();
        let spawned = std::thread::Builder::new()
            .name("ww-fine-timer".into())
            .spawn(move || run(rx));
        if let Err(e) = spawned {
            tracing::warn!("no fine timer thread, accumulation windows are rounded up to ms: {e}");
        }
        tx
    })
}

/// Wakes each waiter at its deadline; a waiter that went away (its sleep was cancelled) is dropped then.
fn run(rx: mpsc::Receiver<Entry>) {
    // ordered by deadline only, the sender is carried along
    struct ByDeadline(Entry);
    impl PartialEq for ByDeadline {
        fn eq(&self, other: &Self) -> bool {
            self.0.0 == other.0.0
        }
    }
    impl Eq for ByDeadline {}
    impl PartialOrd for ByDeadline {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }
    impl Ord for ByDeadline {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            self.0.0.cmp(&other.0.0)
        }
    }

    let mut waiting: BinaryHeap<Reverse<ByDeadline>> = BinaryHeap::new();
    loop {
        let now = Instant::now();
        while waiting.peek().is_some_and(|Reverse(e)| e.0.0 <= now) {
            let Reverse(ByDeadline((_, tx))) = waiting.pop().expect("peeked");
            _ = tx.send(());
        }
        let next = match waiting.peek() {
            Some(Reverse(e)) => rx.recv_timeout(e.0.0 - now),
            None => rx.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected),
        };
        match next {
            Ok(entry) => waiting.push(Reverse(ByDeadline(entry))),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wakes after the deadline, never before; on a paused clock the sleep ends exactly at it.
    #[tokio::test(start_paused = true)]
    async fn paused_clock_ends_at_the_deadline() {
        let t = tokio::time::Instant::now();
        sleep_until(t + Duration::from_micros(1500)).await;
        assert!(t.elapsed() >= Duration::from_micros(1500));
        // tokio's own timer moves the paused clock to the next whole ms
        assert!(t.elapsed() <= Duration::from_millis(2), "{:?}", t.elapsed());
    }

    /// On the wall clock it never wakes before the deadline (how late it wakes depends on the PC's load, which a
    /// test can't bound).
    #[tokio::test]
    async fn never_wakes_early() {
        for us in [0, 50, 300, 900, 2500] {
            let t = tokio::time::Instant::now();
            let deadline = t + Duration::from_micros(us);
            sleep_until(deadline).await;
            assert!(tokio::time::Instant::now() >= deadline, "{us} us");
        }
    }
}

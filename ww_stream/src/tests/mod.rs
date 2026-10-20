mod codec;
mod counted;
mod log_store;
mod persistent_ram;
mod ram_ring;
mod receiver;
mod reliable;
mod sim;
mod sink;
mod sink_sim;

use core::future::Future;
use core::pin::pin;
use core::task::{Context, Poll, Waker};

/// Stores and the sender never wait in these tests: poll until ready.
pub(crate) fn block_on<F: Future>(f: F) -> F::Output {
    let mut f = pin!(f);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut cx) {
            return v;
        }
    }
}

/// Small seeded PRNG (xorshift64*), so every lossy run can be repeated.
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }

    pub(crate) fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// True with `percent` % probability.
    pub(crate) fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    pub(crate) fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next() % (hi - lo + 1)
    }
}

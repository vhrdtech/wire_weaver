//! The hostile block builders and the fuzz targets of ww_tape_fuzz: seed files replayed, hostile streams decoded, each
//! case under a cap on single allocations.

mod builders;
mod known;
mod limits;
mod replay;
mod seeds;
mod stream;

use std::time::Duration;
use ww_tape_fuzz::case::Capped;

#[global_allocator]
static ALLOCATOR: Capped = Capped;

/// The wall-clock time of the cases of a run, for its report only: no outcome depends on it.
#[derive(Default)]
pub struct Times {
    pub total: Duration,
    pub slowest: Duration,
    pub slowest_case: String,
    pub cases: usize,
}

impl Times {
    pub fn add(&mut self, took: Duration, case: impl FnOnce() -> String) {
        self.total += took;
        self.cases += 1;
        if took > self.slowest {
            self.slowest = took;
            self.slowest_case = case();
        }
    }

    pub fn report(&self, what: &str) {
        eprintln!(
            "{what}: {} cases in {:?}, slowest {:?} ({})",
            self.cases, self.total, self.slowest, self.slowest_case
        );
    }
}

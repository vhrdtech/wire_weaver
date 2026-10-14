//! The send scheduler: which stream sends next.
//!
//! Stored streams ([Reliable](crate::tx::Reliable)) don't send when their producer pushes; the event loop sends for
//! them between requests, and [Scheduler] decides the order, in one place:
//!
//! - by [Priority] class, set by the device per stream (a rePort puts its log above a bulk ADC stream, a logger the
//!   other way round), not by the API;
//! - within a class, every live run before any backfill run, so old data never delays new data;
//! - round-robin between streams of one class;
//! - up to a byte budget per turn, so requests are read between turns.
//!
//! Counted and plain streams have nothing stored and write directly when produced; what needs a priority is
//! reliable.
//!
//! ```ignore
//! sched.start_turn();
//! while let Some((i, run)) = sched.pick(&[log.ready(), samples.ready()]) {
//!     let stream = ..; // the i-th stream
//!     match stream.next_event(run, &mut buf).await? {
//!         Some(ev) => { send(ev).await; sched.sent(ev.item.len()); }
//!         None => sched.skip(i, run),
//!     }
//! }
//! ```

use crate::Run;

/// Priority class of a stream, [High](Priority::High) sends first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Priority {
    High,
    #[default]
    Normal,
    Low,
}

impl Priority {
    const ALL: [Priority; 3] = [Priority::High, Priority::Normal, Priority::Low];

    const fn index(self) -> usize {
        self as usize
    }
}

/// What a stream could send now, see [Reliable::ready](crate::tx::Reliable::ready).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StreamReady {
    pub priority: Priority,
    /// The live run has an event and credit.
    pub live: bool,
    /// The backfill run has an event and credit.
    pub backfill: bool,
}

/// Order of sending between up to `N` stored streams, see the [module docs](self). Holds no streams, only whose
/// turn it is: the caller asks [pick](Self::pick) with every stream's [StreamReady], in the same order each time.
#[derive(Debug, Clone)]
pub struct Scheduler<const N: usize> {
    /// Per class and run: the stream after the one picked last
    rr: [[usize; 2]; 3],
    budget: usize,
    spent: usize,
    /// Streams and runs that had nothing this turn after all
    skipped: [[bool; 2]; N],
}

impl<const N: usize> Scheduler<N> {
    /// Send up to `budget_bytes` of items per turn (at least one event).
    pub const fn new(budget_bytes: usize) -> Self {
        Scheduler {
            rr: [[0; 2]; 3],
            budget: budget_bytes,
            spent: 0,
            skipped: [[false; 2]; N],
        }
    }

    /// A new turn: the budget is full again.
    pub fn start_turn(&mut self) {
        self.spent = 0;
        self.skipped = [[false; 2]; N];
    }

    /// Next stream (index into `ready`) and run to send from, `None` when nothing is ready or the turn's budget is
    /// used up.
    pub fn pick(&mut self, ready: &[StreamReady; N]) -> Option<(usize, Run)> {
        if self.spent >= self.budget || N == 0 {
            return None;
        }
        for priority in Priority::ALL {
            for (run_idx, run) in [Run::Live, Run::Backfill].into_iter().enumerate() {
                let start = self.rr[priority.index()][run_idx];
                for k in 0..N {
                    let i = (start + k) % N;
                    let r = &ready[i];
                    let can = match run {
                        Run::Live => r.live,
                        Run::Backfill => r.backfill,
                    };
                    if r.priority == priority && can && !self.skipped[i][run_idx] {
                        self.rr[priority.index()][run_idx] = (i + 1) % N;
                        return Some((i, run));
                    }
                }
            }
        }
        None
    }

    /// An event of `bytes` was sent.
    pub fn sent(&mut self, bytes: usize) {
        self.spent += bytes;
    }

    /// The picked stream had nothing to send after all (no credit for the event, store empty): not picked again
    /// this turn.
    pub fn skip(&mut self, stream: usize, run: Run) {
        if let Some(s) = self.skipped.get_mut(stream) {
            s[match run {
                Run::Live => 0,
                Run::Backfill => 1,
            }] = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(priority: Priority, live: bool, backfill: bool) -> StreamReady {
        StreamReady {
            priority,
            live,
            backfill,
        }
    }

    #[test]
    fn priority_then_live_then_round_robin() {
        let mut s = Scheduler::<4>::new(1000);
        let ready = [
            r(Priority::Low, true, true),
            r(Priority::Normal, true, true),
            r(Priority::High, false, true),
            r(Priority::Normal, true, false),
        ];
        s.start_turn();
        // high class first, though only its backfill run is ready
        assert_eq!(s.pick(&ready), Some((2, Run::Backfill)));
        assert_eq!(s.pick(&ready), Some((2, Run::Backfill)));
        s.skip(2, Run::Backfill);
        // normal class: live runs round-robin, before any backfill
        assert_eq!(s.pick(&ready), Some((1, Run::Live)));
        assert_eq!(s.pick(&ready), Some((3, Run::Live)));
        assert_eq!(s.pick(&ready), Some((1, Run::Live)));
        s.skip(1, Run::Live);
        assert_eq!(s.pick(&ready), Some((3, Run::Live)));
        s.skip(3, Run::Live);
        assert_eq!(s.pick(&ready), Some((1, Run::Backfill)));
        s.skip(1, Run::Backfill);
        // low class last, live before backfill
        assert_eq!(s.pick(&ready), Some((0, Run::Live)));
        s.skip(0, Run::Live);
        assert_eq!(s.pick(&ready), Some((0, Run::Backfill)));
        s.skip(0, Run::Backfill);
        assert_eq!(s.pick(&ready), None);
        // a new turn forgets the skips
        s.start_turn();
        assert_eq!(s.pick(&ready), Some((2, Run::Backfill)));
    }

    #[test]
    fn budget_ends_the_turn() {
        let mut s = Scheduler::<2>::new(100);
        let ready = [
            r(Priority::Normal, true, false),
            r(Priority::Normal, true, false),
        ];
        s.start_turn();
        assert_eq!(s.pick(&ready), Some((0, Run::Live)));
        s.sent(60);
        assert_eq!(s.pick(&ready), Some((1, Run::Live)));
        s.sent(60);
        assert_eq!(s.pick(&ready), None);
        s.start_turn();
        // round-robin goes on across turns
        assert_eq!(s.pick(&ready), Some((0, Run::Live)));
    }

    #[test]
    fn nothing_ready() {
        let mut s = Scheduler::<2>::new(100);
        let ready = [r(Priority::High, false, false), StreamReady::default()];
        s.start_turn();
        assert_eq!(s.pick(&ready), None);
        let mut empty = Scheduler::<0>::new(100);
        assert_eq!(empty.pick(&[]), None);
    }
}

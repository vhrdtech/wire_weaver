//! Benchmarks of `wire_weaver_client`: where the time of a connect goes, step by step, for one client or many in
//! parallel, over simulated USB in this process or a real device.
//!
//! [StepTimes] is a `tracing` layer that times the spans a connect opens ([wire_weaver_client::connect_spans]);
//! [sim] runs blinky devices in this process; the `connect_bench` binary drives both and prints a table per round.

pub mod sim;

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use tracing::Subscriber;
use tracing::span::{Attributes, Id};
use tracing_subscriber::Layer;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use wire_weaver_client::connect_spans as spans;

/// The steps of a connect, in the order the table lists them.
pub(crate) const STEPS: [&str; 7] = [
    spans::CONNECT,
    spans::VALIDATE,
    spans::SELECT,
    spans::LINK,
    spans::OPEN,
    spans::LEARN,
    spans::INTROSPECT,
];

/// A `tracing` layer that records how long each connect step's span lived, from creation to close.
#[derive(Clone, Default)]
pub struct StepTimes {
    done: Arc<Mutex<Vec<(&'static str, Duration)>>>,
}

/// When a step's span was created, in its extensions.
struct Started(Instant);

impl StepTimes {
    /// The steps closed since the last call, by name, in the order they closed.
    pub fn take(&self) -> Vec<(&'static str, Duration)> {
        std::mem::take(&mut *self.done())
    }

    /// The closed steps; a panic elsewhere while they were held left whole entries only.
    fn done(&self) -> MutexGuard<'_, Vec<(&'static str, Duration)>> {
        self.done.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl<S: Subscriber + for<'a> LookupSpan<'a>> Layer<S> for StepTimes {
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        if !STEPS.contains(&attrs.metadata().name()) {
            return;
        }
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(Started(Instant::now()));
        }
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else { return };
        let Some(started) = span.extensions().get::<Started>().map(|s| s.0) else {
            return;
        };
        self.done().push((span.name(), started.elapsed()));
    }
}

/// Statistics of one step over a round's connects.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stats {
    /// How many times the step ran
    pub count: usize,
    pub median: Duration,
    pub max: Duration,
    /// Sum over every run: more than the round's wall time when connects overlapped
    pub sum: Duration,
}

/// The [Stats] of `step` in `times`, `None` when it never ran.
pub fn stats(times: &[(&'static str, Duration)], step: &str) -> Option<Stats> {
    let mut of_step: Vec<Duration> = times
        .iter()
        .filter(|(name, _)| *name == step)
        .map(|(_, d)| *d)
        .collect();
    of_step.sort_unstable();
    let median = *of_step.get(of_step.len() / 2)?;
    Some(Stats {
        count: of_step.len(),
        median,
        max: *of_step.last()?,
        sum: of_step.iter().sum(),
    })
}

/// A table of every step that ran in `times`: count, median, max and sum in ms.
pub fn table(times: &[(&'static str, Duration)]) -> String {
    let ms = |d: Duration| d.as_secs_f64() * 1e3;
    let mut out = format!(
        "{:<22} {:>5} {:>10} {:>10} {:>10}\n",
        "step", "n", "median ms", "max ms", "sum ms"
    );
    for step in STEPS {
        if let Some(s) = stats(times, step) {
            out += &format!(
                "{step:<22} {:>5} {:>10.2} {:>10.2} {:>10.1}\n",
                s.count,
                ms(s.median),
                ms(s.max),
                ms(s.sum)
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_of_a_step() {
        let ms = Duration::from_millis;
        let times = [
            (spans::LINK, ms(3)),
            (spans::CONNECT, ms(10)),
            (spans::LINK, ms(1)),
            (spans::LINK, ms(2)),
        ];
        assert_eq!(
            stats(&times, spans::LINK),
            Some(Stats {
                count: 3,
                median: ms(2),
                max: ms(3),
                sum: ms(6)
            })
        );
        assert_eq!(stats(&times, spans::OPEN), None);
    }
}

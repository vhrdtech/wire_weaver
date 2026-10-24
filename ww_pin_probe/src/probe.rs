//! The tests: pulls, shorts to GND and to the supply per point, then shorts between points.
//!
//! Per point, from what its caps (narrowed by a plan) allow:
//!
//! * **pulls**: drive low, then input with the internal pull-up, read; drive high, then input with the pull-down,
//!   read. Driving the opposite level first keeps an input latch (RP2350-E9) from holding a stale level. High with
//!   the pull-up and low with the pull-down: floating; high with both: an external pull-up (held high when the
//!   short to the supply test says so); low with both: an external pull-down (or held low).
//! * **short to GND**: drive high with [Drive::Weak], read back: low means shorted to GND, an external pull-down
//!   doesn't win against even a weak drive.
//! * **short to the supply**: drive low with [Drive::Weak], read back: high means shorted to the supply.
//!
//! Between the points of [Setup::among], twice: once with every point on its pull-up, each source driven low alone,
//! once with every point on its pull-down, each source driven high alone. A point that reads the driven level and
//! didn't before (at rest) is connected to the source. Points that can only drive (shift register outputs) rest
//! low, so they are sources in the high round only; points that can't be pulled are sensors where their level at
//! rest is the opposite of the driven one. `--watch` points are only read: their level once, and as sensors.
//!
//! What a point can't do is reported as [Check::NotChecked] with the reason, a refusal as [Check::Refused]: the
//! tests go on. [run] gives every point back with `release_all` at the end, also when cancelled or failed.

use crate::plan::{Plan, PullClass};
use crate::{Drive, Level, PinError, Pins, Point, PointCaps, PointState, Pull, refusal};
use anyhow::{Result, anyhow};
use std::collections::BTreeSet;
use std::fmt::{Display, Formatter};

/// One point under test, its caps narrowed by the plan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub point: Point,
    /// The device's caps narrowed by the plan's `allow`; for a watched point only `read`.
    pub caps: PointCaps,
    /// Only read, never set (`--watch`).
    pub watch: bool,
    /// The plan's `expect`.
    pub expect: Option<PullClass>,
}

/// What to test: the points, which of them are tested against each other, which are one net.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Setup {
    pub targets: Vec<Target>,
    /// Ids of the points tested against each other.
    pub among: Vec<u16>,
    /// Groups of ids that are one net: a connection between them is expected, a missing one is a fault.
    pub same_net: Vec<Vec<u16>>,
}

impl Setup {
    /// `selected` and `watch` are point ids (see [crate::select::select]). Without either, the points the plan
    /// names are tested, without a plan all points. `--watch` wins over `selected` for a point in both.
    pub fn new(
        points: &[Point],
        selected: &[u16],
        watch: &[u16],
        plan: Option<&Plan>,
    ) -> Result<Setup> {
        let resolved = plan.map(|p| p.resolve(points)).transpose()?;
        let mut selected = selected.to_vec();
        // no points given: the plan's, also next to --watch; without a plan all, but only when nothing is
        // watched (--watch alone reads, it doesn't set every point)
        if selected.is_empty() {
            match &resolved {
                Some(r) if !r.mentioned.is_empty() => selected = r.mentioned.clone(),
                _ if watch.is_empty() => selected = points.iter().map(|p| p.id).collect(),
                _ => {}
            }
        }
        let point = |id: u16| {
            points
                .iter()
                .find(|p| p.id == id)
                .ok_or_else(|| anyhow!("no point P{id}"))
        };
        let mut targets = vec![];
        for &id in selected.iter().filter(|id| !watch.contains(id)) {
            let point = point(id)?.clone();
            let mut caps = point.caps;
            let mut expect = None;
            if let Some(r) = &resolved {
                if let Some(allow) = r.allow.get(&id) {
                    caps = narrow(caps, *allow);
                }
                expect = r.expect.get(&id).copied();
            }
            targets.push(Target {
                caps,
                point,
                watch: false,
                expect,
            });
        }
        for &id in watch {
            let point = point(id)?.clone();
            let caps = PointCaps {
                read: point.caps.read,
                pull_up: false,
                pull_down: false,
                drive_low: false,
                drive_high: false,
                weak_drive: false,
            };
            targets.push(Target {
                caps,
                point,
                watch: true,
                expect: None,
            });
        }
        let ids: Vec<u16> = targets.iter().map(|t| t.point.id).collect();
        let among = match resolved.as_ref().and_then(|r| r.among.as_ref()) {
            Some(among) => among
                .iter()
                .copied()
                .filter(|id| ids.contains(id))
                .collect(),
            None => ids,
        };
        let same_net = resolved.map(|r| r.same_net).unwrap_or_default();
        Ok(Setup {
            targets,
            among,
            same_net,
        })
    }
}

/// `caps` with only what `allow` allows too.
pub fn narrow(caps: PointCaps, allow: PointCaps) -> PointCaps {
    PointCaps {
        read: caps.read && allow.read,
        pull_up: caps.pull_up && allow.pull_up,
        pull_down: caps.pull_down && allow.pull_down,
        drive_low: caps.drive_low && allow.drive_low,
        drive_high: caps.drive_high && allow.drive_high,
        weak_drive: caps.weak_drive,
    }
}

/// Outcome of one check of one point.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Check<T> {
    Done(T),
    /// The point can't do what the check needs (its caps or the plan), or isn't part of it: the reason.
    NotChecked(String),
    /// The device refused a step: what it said, as `<step>: <refusal>`, several joined with `; `.
    Refused(String),
}

impl<T> Check<T> {
    pub fn done(&self) -> Option<&T> {
        match self {
            Check::Done(t) => Some(t),
            _ => None,
        }
    }
}

/// Everything a run found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub points: Vec<PointReport>,
    /// Pairs of points found connected, each once, the lower id first.
    pub connections: Vec<Connection>,
    /// Pairs of a `same_net` group that were tested against each other and not found connected.
    pub missing: Vec<(u16, u16)>,
}

/// What a run found on one point.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PointReport {
    pub target: Target,
    /// Level of a watched point, read before anything was set.
    pub level: Option<Check<Level>>,
    pub pulls: Check<PullClass>,
    /// `true`: shorted to GND.
    pub gnd: Check<bool>,
    /// `true`: shorted to the supply.
    pub vdd: Check<bool>,
    /// Whether the point took part in the test between points.
    pub between: Check<()>,
    /// Refusals on the way that didn't decide a check on their own.
    pub notes: Vec<String>,
}

/// Two points found connected.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Connection {
    pub a: u16,
    pub b: u16,
    /// Both are in one `same_net` group of the plan.
    pub expected: bool,
}

/// A point whose pull class isn't what the plan expects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mismatch {
    pub id: u16,
    pub expected: PullClass,
    /// What the test found, or why it found nothing.
    pub found: Result<PullClass, String>,
}

impl Report {
    pub fn point(&self, id: u16) -> Option<&PointReport> {
        self.points.iter().find(|p| p.target.point.id == id)
    }

    /// Shorts to GND or the supply, and connections the plan doesn't expect.
    pub fn shorts(&self) -> usize {
        let rails = self
            .points
            .iter()
            .map(|p| (p.gnd == Check::Done(true)) as usize + (p.vdd == Check::Done(true)) as usize)
            .sum::<usize>();
        rails + self.connections.iter().filter(|c| !c.expected).count()
    }

    /// Points whose pulls aren't what the plan expects, also when they couldn't be tested.
    pub fn mismatches(&self) -> Vec<Mismatch> {
        let mut out = vec![];
        for p in &self.points {
            let Some(expected) = p.target.expect else {
                continue;
            };
            let found = match &p.pulls {
                Check::Done(c) if *c == expected => continue,
                Check::Done(c) => Ok(*c),
                Check::NotChecked(why) => Err(format!("not checked: {why}")),
                Check::Refused(why) => Err(why.clone()),
            };
            out.push(Mismatch {
                id: p.target.point.id,
                expected,
                found,
            });
        }
        out
    }

    /// No short, no unexpected connection, every expectation met, every `same_net` pair connected.
    pub fn passed(&self) -> bool {
        self.shorts() == 0 && self.mismatches().is_empty() && self.missing.is_empty()
    }
}

/// Why [run] stopped early.
#[derive(Debug)]
pub enum Stopped {
    /// The `cancel` future finished (Ctrl-C). Every point was given back.
    Cancelled,
    /// A call failed (link, timeout): the error, and whether `release_all` went through after it.
    Failed { error: PinError, released: bool },
}

impl Display for Stopped {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Stopped::Cancelled => write!(f, "cancelled, all points released"),
            Stopped::Failed { error, released } => {
                write!(f, "{error}")?;
                if !released {
                    write!(
                        f,
                        " (and release_all failed: reset the device to give the points back)"
                    )?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for Stopped {}

/// Runs the tests of `setup` and gives every point back with `release_all`, also when `cancel` finishes first
/// (pass `tokio::signal::ctrl_c()`, or `std::future::pending()`) or a call fails. `progress` gets the steps done
/// and the steps in all.
pub async fn run<P: Pins>(
    pins: &mut P,
    setup: &Setup,
    cancel: impl Future,
    progress: impl FnMut(usize, usize),
) -> Result<Report, Stopped> {
    let outcome = tokio::select! {
        // a cancel already there wins, not a coin toss
        biased;
        _ = cancel => None,
        r = test(pins, setup, progress) => Some(r),
    };
    let released = pins.release_all().await;
    match outcome {
        None => match released {
            Ok(()) => Err(Stopped::Cancelled),
            Err(error) => Err(Stopped::Failed {
                error,
                released: false,
            }),
        },
        Some(Ok(report)) => match released {
            Ok(()) => Ok(report),
            Err(error) => Err(Stopped::Failed {
                error,
                released: false,
            }),
        },
        Some(Err(error)) => Err(Stopped::Failed {
            error,
            released: released.is_ok(),
        }),
    }
}

/// The tests without the release at the end: use [run].
async fn test<P: Pins>(
    pins: &mut P,
    setup: &Setup,
    progress: impl FnMut(usize, usize),
) -> Result<Report, PinError> {
    let mut t = Tester {
        pins,
        setup,
        report: Report::default(),
        progress,
        done: 0,
        total: 0,
    };
    t.total = setup.targets.iter().filter(|t| !t.watch).count()
        + 2 * t.sources().len()
        + setup.targets.iter().filter(|t| t.watch).count();
    for target in &setup.targets {
        t.report.points.push(PointReport {
            target: target.clone(),
            level: None,
            pulls: Check::NotChecked("watched, only read".into()),
            gnd: Check::NotChecked("watched, only read".into()),
            vdd: Check::NotChecked("watched, only read".into()),
            between: Check::NotChecked("not in [shorts] among".into()),
            notes: vec![],
        });
    }
    // watched points first, before anything was set
    for i in 0..setup.targets.len() {
        if setup.targets[i].watch {
            let level = match t.read(i).await? {
                Ok(level) => Check::Done(level),
                Err(why) => Check::Refused(why),
            };
            t.report.points[i].level = Some(level);
            t.step();
        }
    }
    for i in 0..setup.targets.len() {
        if !setup.targets[i].watch {
            t.per_point(i).await?;
            t.step();
        }
    }
    t.between().await?;
    Ok(t.report)
}

struct Tester<'a, P, F> {
    pins: &'a mut P,
    setup: &'a Setup,
    report: Report,
    progress: F,
    done: usize,
    total: usize,
}

/// A step's outcome: `Ok(Err(why))` is a refusal, `Err` a failed call.
type Step<T> = Result<Result<T, String>, PinError>;

impl<P: Pins, F: FnMut(usize, usize)> Tester<'_, P, F> {
    fn step(&mut self) {
        self.done += 1;
        (self.progress)(self.done.min(self.total), self.total);
    }

    fn caps(&self, i: usize) -> PointCaps {
        self.setup.targets[i].caps
    }

    fn id(&self, i: usize) -> u16 {
        self.setup.targets[i].point.id
    }

    async fn set(&mut self, i: usize, state: PointState) -> Step<()> {
        match self.pins.set(self.id(i), state).await {
            Ok(()) => Ok(Ok(())),
            Err(PinError::Refused(e)) => Ok(Err(format!("{}: {}", describe(state), refusal(&e)))),
            Err(e) => Err(e),
        }
    }

    async fn read(&mut self, i: usize) -> Step<Level> {
        match self.pins.read(self.id(i)).await {
            Ok(level) => Ok(Ok(level)),
            Err(PinError::Refused(e)) => Ok(Err(format!("read: {}", refusal(&e)))),
            Err(e) => Err(e),
        }
    }

    /// Sets, then reads; a refusal of either is the outcome.
    async fn set_read(&mut self, i: usize, state: PointState) -> Step<Level> {
        if let Err(why) = self.set(i, state).await? {
            return Ok(Err(why));
        }
        self.read(i).await
    }

    fn note(&mut self, i: usize, why: String) {
        let notes = &mut self.report.points[i].notes;
        if !notes.contains(&why) {
            notes.push(why);
        }
    }

    async fn per_point(&mut self, i: usize) -> Result<(), PinError> {
        let c = self.caps(i);
        if !c.read {
            let why = "can't be read".to_string();
            let p = &mut self.report.points[i];
            p.pulls = Check::NotChecked(why.clone());
            p.gnd = Check::NotChecked(why.clone());
            p.vdd = Check::NotChecked(why);
            return Ok(());
        }
        // pulls, each after driving the opposite level where allowed
        let mut refusals = vec![];
        let up = self
            .pulled(
                i,
                Pull::Up,
                c.pull_up,
                c.drive_low.then_some(Level::Low),
                &mut refusals,
            )
            .await?;
        let down = self
            .pulled(
                i,
                Pull::Down,
                c.pull_down,
                c.drive_high.then_some(Level::High),
                &mut refusals,
            )
            .await?;
        let mut pulls = classify(up, down, c, &refusals);

        let gnd = self.rail(i, c.drive_high, Level::High).await?;
        let vdd = self.rail(i, c.drive_low, Level::Low).await?;
        if let Err(why) = self.set(i, PointState::Input { pull: Pull::None }).await? {
            self.note(i, why);
        }
        pulls = match pulls {
            Check::Done(PullClass::PullUp) if vdd == Check::Done(true) => {
                Check::Done(PullClass::HeldHigh)
            }
            Check::Done(PullClass::PullDown) if gnd == Check::Done(true) => {
                Check::Done(PullClass::HeldLow)
            }
            other => other,
        };
        let p = &mut self.report.points[i];
        p.pulls = pulls;
        p.gnd = gnd;
        p.vdd = vdd;
        Ok(())
    }

    /// Level with the internal `pull`, after driving `pre` first; `None` when not capable or refused.
    async fn pulled(
        &mut self,
        i: usize,
        pull: Pull,
        capable: bool,
        pre: Option<Level>,
        refusals: &mut Vec<String>,
    ) -> Result<Option<Level>, PinError> {
        if !capable {
            return Ok(None);
        }
        if let Some(level) = pre
            && let Err(why) = self.set(i, output(level, Drive::Weak)).await?
        {
            self.note(i, why);
        }
        Ok(match self.set_read(i, PointState::Input { pull }).await? {
            Ok(level) => Some(level),
            Err(why) => {
                refusals.push(why);
                None
            }
        })
    }

    /// Drives `level` weakly and reads back: `Done(true)` when the pad shows the other level.
    async fn rail(
        &mut self,
        i: usize,
        capable: bool,
        level: Level,
    ) -> Result<Check<bool>, PinError> {
        if !capable {
            return Ok(Check::NotChecked(match level {
                Level::High => "can't drive high".into(),
                Level::Low => "can't drive low".into(),
            }));
        }
        Ok(match self.set_read(i, output(level, Drive::Weak)).await? {
            Ok(read) => Check::Done(read != level),
            Err(why) => Check::Refused(why),
        })
    }

    /// Indices of the targets among [Setup::among].
    fn among(&self) -> Vec<usize> {
        (0..self.setup.targets.len())
            .filter(|&i| self.setup.among.contains(&self.id(i)))
            .collect()
    }

    fn sources(&self) -> Vec<usize> {
        self.among()
            .into_iter()
            .filter(|&i| {
                let t = &self.setup.targets[i];
                !t.watch && (t.caps.drive_low || t.caps.drive_high)
            })
            .collect()
    }

    /// Where a point rests in the round driving `level`: on the opposite pull if it can, an input without one
    /// otherwise; a point that only drives rests low. `None`: not set at all (watched, or nothing it can do).
    fn rest(&self, i: usize, level: Level) -> Option<PointState> {
        let t = &self.setup.targets[i];
        let c = t.caps;
        if t.watch {
            None
        } else if c.read {
            let pull = match level {
                Level::Low if c.pull_up => Pull::Up,
                Level::High if c.pull_down => Pull::Down,
                _ => Pull::None,
            };
            Some(PointState::Input { pull })
        } else if c.drive_low {
            Some(output(Level::Low, Drive::Normal))
        } else {
            None
        }
    }

    async fn between(&mut self) -> Result<(), PinError> {
        let among = self.among();
        let sources = self.sources();
        let mut connected = BTreeSet::new();
        let mut tested = BTreeSet::new();
        let mut took_part = vec![false; self.setup.targets.len()];
        for level in [Level::Low, Level::High] {
            // everyone at rest, then the levels at rest
            let mut usable = vec![];
            for &i in &among {
                if let Some(state) = self.rest(i, level)
                    && let Err(why) = self.set(i, state).await?
                {
                    self.note(i, why);
                    continue;
                }
                if !self.caps(i).read {
                    continue;
                }
                match self.read(i).await? {
                    Ok(at_rest) if at_rest != level => usable.push(i),
                    Ok(_) => {}
                    Err(why) => self.note(i, why),
                }
            }
            for &s in &sources {
                let c = self.caps(s);
                let can = match level {
                    Level::Low => c.drive_low && c.read,
                    Level::High => c.drive_high,
                };
                if !can {
                    self.step();
                    continue;
                }
                if let Err(why) = self.set(s, output(level, Drive::Normal)).await? {
                    self.note(s, why);
                    self.step();
                    continue;
                }
                for &t in usable.iter().filter(|&&t| t != s) {
                    let pair = pair(self.id(s), self.id(t));
                    tested.insert(pair);
                    took_part[s] = true;
                    took_part[t] = true;
                    match self.read(t).await? {
                        Ok(read) if read == level => {
                            connected.insert(pair);
                        }
                        Ok(_) => {}
                        Err(why) => self.note(t, why),
                    }
                }
                if let Some(state) = self.rest(s, level)
                    && let Err(why) = self.set(s, state).await?
                {
                    self.note(s, why);
                }
                self.step();
            }
        }
        for &i in &among {
            let c = self.caps(i);
            let p = &mut self.report.points[i];
            p.between = if took_part[i] {
                Check::Done(())
            } else if !c.read && !c.drive_low && !c.drive_high {
                Check::NotChecked("can't drive or be read".into())
            } else if !p.notes.is_empty() {
                Check::Refused(p.notes.join("; "))
            } else if among.len() < 2 {
                Check::NotChecked("no other point to test against".into())
            } else {
                Check::NotChecked("no pair it could tell".into())
            };
        }
        let same_net = |a: u16, b: u16| {
            self.setup
                .same_net
                .iter()
                .any(|net| net.contains(&a) && net.contains(&b))
        };
        self.report.connections = connected
            .iter()
            .map(|&(a, b)| Connection {
                a,
                b,
                expected: same_net(a, b),
            })
            .collect();
        for net in &self.setup.same_net {
            for (k, &a) in net.iter().enumerate() {
                for &b in &net[k + 1..] {
                    let p = pair(a, b);
                    if tested.contains(&p) && !connected.contains(&p) {
                        self.report.missing.push(p);
                    }
                }
            }
        }
        Ok(())
    }
}

fn pair(a: u16, b: u16) -> (u16, u16) {
    (a.min(b), a.max(b))
}

fn output(level: Level, drive: Drive) -> PointState {
    PointState::Output { level, drive }
}

/// `pull-up`, `high (weak)`: a state in a refusal note.
fn describe(state: PointState) -> String {
    match state {
        PointState::Input { pull: Pull::Up } => "pull-up".into(),
        PointState::Input { pull: Pull::Down } => "pull-down".into(),
        PointState::Input { pull: Pull::None } => "input".into(),
        PointState::Output { level, drive } => {
            let level = match level {
                Level::High => "high",
                Level::Low => "low",
            };
            match drive {
                Drive::Weak => format!("drive {level} (weak)"),
                _ => format!("drive {level}"),
            }
        }
        other => format!("{other:?}"),
    }
}

/// The pull class from the level with the internal pull-up and with the pull-down, where they were read.
fn classify(
    up: Option<Level>,
    down: Option<Level>,
    c: PointCaps,
    refusals: &[String],
) -> Check<PullClass> {
    use Level::*;
    let refused = || Check::Refused(refusals.join("; "));
    match (up, down) {
        (Some(High), Some(Low)) => Check::Done(PullClass::Floating),
        (Some(High), Some(High)) | (None, Some(High)) => Check::Done(PullClass::PullUp),
        (Some(Low), Some(Low)) | (Some(Low), None) => Check::Done(PullClass::PullDown),
        (Some(Low), Some(High)) => Check::Done(PullClass::Odd),
        _ if !refusals.is_empty() => refused(),
        (Some(High), None) => Check::NotChecked(
            "high on the pull-up, no pull-down to tell an external pull-up from floating".into(),
        ),
        (None, Some(Low)) => Check::NotChecked(
            "low on the pull-down, no pull-up to tell an external pull-down from floating".into(),
        ),
        _ if !c.pull_up && !c.pull_down => Check::NotChecked("no internal pulls".into()),
        _ => Check::NotChecked("pulls not read".into()),
    }
}

#[cfg(test)]
mod tests;

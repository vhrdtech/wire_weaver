//! Observed properties on the device (SRV-18): what the generated `poll_properties` and the `Observe` request
//! handler of a server run on.
//!
//! A host observes a trait level's `+observe` properties with `RequestKind::Observe` (API-38,
//! [ww_client_server::observe]); the device keeps one [ObserveTable] per level, sized at compile time, and sends one
//! `EventKind::PropertyUpdate` per level whenever any of its properties is due, carrying just those.
//!
//! - **Due**: `Periodic` every period on a fixed grid; `OnChange` when marked changed (`mark_changed_<name>()`, or a
//!   write to a `value_on_changed` property) and at least `min` after the previous send; `OnChangeOrBeat` also when
//!   `max` passed without a send. A property that starts being observed is sent once right away, so the host has a
//!   value to start from.
//! - **Budget**: a token bucket per level, filled at the granted bytes per second, holding at most a tenth of a
//!   second of it ([MIN_BURST] at least). An update is sent only when it fits the bucket and the buffer; due
//!   properties go in earliest deadline first (a property left out keeps its deadline until it is sent, so none
//!   starves). When not all of them fit the bucket and it isn't full yet, nothing is sent: the level holds until
//!   they all fit or the bucket is full, so a level short of budget sends fewer, fuller updates instead of one
//!   value per poll (each update has a header of its own). From a full bucket, or when the buffer is what limits,
//!   the first property that doesn't fit ends the update and it and the rest slip to the next poll. A property too
//!   big for the bucket's capacity goes alone when the bucket is full (the bucket goes negative for it, the rate
//!   over time still holds).
//! - **Missed**: a periodic property counts one missed send for every whole period it fell behind, an on-change
//!   one when it was held back or left out for its `min` or longer (once until it is sent). Held back for less,
//!   it is only late, so polling often doesn't inflate the count. A send skipped because the value could not be
//!   read or serialized counts too. The count goes to the host in the next update's `missed`.
//! - **Time**: `t` of an update is the device's microseconds as a `u32` (`Second { Micro, U32 }`, 5 bytes), it
//!   wraps every 71.6 minutes; the host unwraps it.
//! - **Every sample (API-39)**: a `+batched` property observed in `Samples` mode is due whenever its level's
//!   outbox (`ww_stream::batch::Outbox`) holds a batch of it; the generated `poll_properties` encodes sealed
//!   batches into the outbox first, then sends updates while batches wait and the budget lasts, more than one per
//!   call. Samples dropped on the way (the outbox full, the event loop behind the sampler) count as missed. The
//!   sampler (generated `<Trait>Sampler`, in the producer's context, an interrupt maybe) asks a [SampleGate] per
//!   property whether to take a sample: a load of an atomic, set by the event loop from the host's mode.
//! - **Untrusted input**: [ObserveTable::apply] validates an `ObserveRequest` completely before it changes
//!   anything (every entry decoded, index known, `+observe`, no duplicate, mode known), clamps periods and the
//!   budget to [ObserveLimits], and does work linear in the request's length; a malformed request changes nothing.
//!
//! **Sharing the link with stored streams.** Properties have a reserved share: all levels together are granted at
//! most [ObserveLimits::total_budget] bytes per second, and the generated `poll_properties` sends within it. Stored
//! (reliable) streams take what `poll_streams` gets per turn, after the properties. Call `poll_properties` before
//! `poll_streams` in the event loop: properties can't starve streams beyond their share, and streams can't starve
//! properties, whose deadlines are short.

use portable_atomic::{AtomicU32, Ordering};
use wire_weaver::shrink_wrap::{Error as ShrinkWrapError, SerializeShrinkWrap, UNib32};
use ww_client_server::observe::{
    ObserveEntry, ObserveMode, ObserveRefusal, ObserveRefused, ObserveRequest,
};

/// Smallest capacity of a level's token bucket in bytes, so that a level with a small budget can still send an
/// update of a few properties at once.
pub const MIN_BURST: u32 = 64;

/// Room left in an update's size estimate for the event's own length fields, which grow with the data.
const EVENT_SLACK: usize = 4;

/// Length bytes a value's size can take in an `OptionAnyVec` (a `UNib32` of a `u32` length, rounded up).
const LEN_SLOT: usize = 6;

/// Token bucket fractions per byte: tokens are kept in byte-microseconds.
const SCALE: i64 = 1_000_000;

/// What the device allows observers, applied to every `Observe` request. Set by the firmware once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObserveLimits {
    /// Shortest period and shortest `min` between two sends of one property, in microseconds.
    pub min_period_us: u32,
    /// Bytes per second a level gets when the host leaves the budget to the device.
    pub default_budget: u32,
    /// Most bytes per second one level may be granted.
    pub max_level_budget: u32,
    /// Most bytes per second all levels together may be granted: the properties' share of the link.
    pub total_budget: u32,
    /// Put the device time (`t`, microseconds of the `now_us` given to `poll_properties`) into every update.
    pub timestamps: bool,
    /// Least and most `max_age_us` of `Samples` mode (API-39), µs: how long batches of samples may wait on the
    /// device. A batch is also sent when it is full, so the most only matters for slow samples.
    pub samples_max_age_us: (u32, u32),
    /// Shortest `every_us` of `Samples` mode (API-39), µs: each property's sampler takes at most one sample this
    /// often, whatever a host asks for. Every sample taken is encoded, also when the link can't carry it, so this
    /// is what keeps the work a host can ask for within the device's CPU (about 2000 instructions per sample on a
    /// Cortex-M3, see `docs/api/observing.md`). 0: every sample.
    pub samples_min_every_us: u32,
}

impl ObserveLimits {
    /// 1 ms shortest period, 2 kB/s per level by default, 20 kB/s per level at most, 50 kB/s in total, no
    /// timestamps, batches of samples 1 ms to 10 s old, every sample (set `samples_min_every_us` on an MCU).
    pub const DEFAULT: ObserveLimits = ObserveLimits {
        min_period_us: 1_000,
        default_budget: 2_000,
        max_level_budget: 20_000,
        total_budget: 50_000,
        timestamps: false,
        samples_max_age_us: (crate::batch::MIN_MAX_AGE_US, crate::batch::MAX_MAX_AGE_US),
        samples_min_every_us: 0,
    };
}

impl Default for ObserveLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The observable properties of one trait level, generated: `observable[k]` is the resource index of the
/// property in slot `k` of the level's [ObserveTable], increasing.
#[derive(Debug, Clone, Copy)]
pub struct LevelDesc<const N: usize> {
    /// Number of resource indices of the level (largest one + 1), properties or not.
    pub item_count: u32,
    pub observable: [u32; N],
    /// `+batched` (API-39): the slot takes `Samples` mode, its value is written as a `Batched`.
    pub batched: [bool; N],
    /// Resource index of the level's `sample_times` slot (API-39, shared clock): the time column of the samples in
    /// an update, past the level's last resource index. `None` for a level without `+batched` properties.
    pub time_slot: Option<u32>,
}

impl<const N: usize> LevelDesc<N> {
    fn slot_of(&self, index: u32) -> Option<usize> {
        self.observable.binary_search(&index).ok()
    }

    /// Upper bound of the presence header of an update: never longer than the dense mode's, a mode nibble,
    /// the `UNib32` slot count and a bit per slot.
    fn header_bound(&self) -> usize {
        let last = self.observable.last().copied().max(self.time_slot);
        let len = last.map_or(0, |last| last + 1);
        let len_nibbles = (32 - len.leading_zeros()).div_ceil(3).max(1) as usize;
        (4 + 4 * len_nibbles + len as usize).div_ceil(8)
    }
}

/// How one property is observed, periods clamped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Mode {
    #[default]
    Off,
    OnChange {
        min: u32,
    },
    Periodic {
        every: u32,
    },
    OnChangeOrBeat {
        min: u32,
        max: u32,
    },
    /// Every sample (`every` 0), or one per `every` µs, in batches at most `max_age` µs old.
    Samples {
        every: u32,
        max_age: u32,
    },
}

impl Mode {
    /// None for a mode this device doesn't know, and for `Samples` of a slot that isn't `batched`.
    fn clamped(mode: &ObserveMode, limits: &ObserveLimits, batched: bool) -> Option<Mode> {
        let floor = limits.min_period_us.max(1);
        Some(match mode {
            ObserveMode::Off => Mode::Off,
            ObserveMode::OnChange { min_us } => Mode::OnChange {
                min: min_us.0.max(floor),
            },
            ObserveMode::Periodic { every_us } => Mode::Periodic {
                every: every_us.0.max(floor),
            },
            ObserveMode::OnChangeOrBeat { min_us, max_us } => {
                let min = min_us.0.max(floor);
                Mode::OnChangeOrBeat {
                    min,
                    max: max_us.0.max(min),
                }
            }
            ObserveMode::Samples { .. } if !batched => return None,
            ObserveMode::Samples {
                every_us,
                max_age_us,
            } => {
                let (lo, hi) = limits.samples_max_age_us;
                let lo = lo.max(1);
                Mode::Samples {
                    // 0 is every sample; decimating never sends more than that, so the device's floor is all
                    // there is to check
                    every: every_us.0.max(limits.samples_min_every_us),
                    max_age: max_age_us.0.clamp(lo, hi.max(lo)),
                }
            }
            ObserveMode::Unknown(_) => return None,
        })
    }

    fn to_wire(self) -> ObserveMode {
        match self {
            Mode::Off => ObserveMode::Off,
            Mode::OnChange { min } => ObserveMode::OnChange {
                min_us: UNib32(min),
            },
            Mode::Periodic { every } => ObserveMode::Periodic {
                every_us: UNib32(every),
            },
            Mode::OnChangeOrBeat { min, max } => ObserveMode::OnChangeOrBeat {
                min_us: UNib32(min),
                max_us: UNib32(max),
            },
            Mode::Samples { every, max_age } => ObserveMode::Samples {
                every_us: UNib32(every),
                max_age_us: UNib32(max_age),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Slot {
    mode: Mode,
    /// Periodic: the next send on the grid.
    next_due: u64,
    last_sent: Option<u64>,
    /// Changed since the last send.
    dirty: bool,
    /// Due and left out of an update, already counted as missed.
    slipped: bool,
    /// Deadline of the oldest due send that was left out, until a send: what earliest deadline first goes by, so
    /// that a property waiting longer goes first even after its period moved on.
    waiting: Option<u64>,
    /// Just started being observed: sent once at the next poll.
    start: bool,
}

/// Why an `Observe` request was refused as a whole, nothing applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObserveError {
    /// An entry could not be read.
    Malformed(ShrinkWrapError),
    /// More entries refused than the answer has room for.
    TooManyRefused,
}

/// Observe settings and send state of one trait level with `N` observable properties.
#[derive(Debug, Clone)]
pub struct ObserveTable<const N: usize> {
    slots: [Slot; N],
    /// Granted bytes per second, 0 while nothing is observed.
    budget: u32,
    /// Byte-microseconds in the bucket.
    tokens: i64,
    last_refill: Option<u64>,
    /// Missed sends not reported to the host yet.
    missed: u32,
}

impl<const N: usize> Default for ObserveTable<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> ObserveTable<N> {
    /// Nothing observed.
    pub const fn new() -> Self {
        ObserveTable {
            slots: [Slot {
                mode: Mode::Off,
                next_due: 0,
                last_sent: None,
                dirty: false,
                slipped: false,
                waiting: None,
                start: false,
            }; N],
            budget: 0,
            tokens: 0,
            last_refill: None,
            missed: 0,
        }
    }

    /// Stop observing everything, e.g., when the host is gone.
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// The property in slot `k` changed: sent at its next chance if observed on change.
    pub fn mark_changed(&mut self, k: usize) {
        if let Some(slot) = self.slots.get_mut(k) {
            slot.dirty = true;
        }
    }

    /// Bytes per second granted to this level, 0 when nothing is observed.
    pub fn budget(&self) -> u32 {
        self.budget
    }

    /// Whether any property of the level is observed.
    pub fn is_active(&self) -> bool {
        self.slots.iter().any(|s| s.mode != Mode::Off)
    }

    /// Apply an `Observe` request from the host: validate all of it first (nothing changes when it fails), then
    /// set the modes, clamped to `limits`, and grant a budget within what `others_budget` (granted to the other
    /// levels) leaves of [ObserveLimits::total_budget]. Refused entries are put into `refused`, their count is
    /// returned.
    pub fn apply(
        &mut self,
        req: &ObserveRequest<'_>,
        desc: &LevelDesc<N>,
        limits: &ObserveLimits,
        others_budget: u32,
        refused: &mut [ObserveRefused],
    ) -> Result<usize, ObserveError> {
        // first pass: check everything, change nothing
        let mut seen = [false; N];
        let mut refused_len = 0;
        for entry in req.entries.iter() {
            let entry = entry.map_err(ObserveError::Malformed)?;
            let reason = match desc.slot_of(entry.index.0) {
                None if entry.index.0 >= desc.item_count => Some(ObserveRefusal::NoSuchIndex),
                None => Some(ObserveRefusal::NotObservable),
                Some(k) if seen[k] => Some(ObserveRefusal::Duplicate),
                Some(k) => {
                    seen[k] = true;
                    Mode::clamped(&entry.mode, limits, desc.batched[k])
                        .is_none()
                        .then_some(ObserveRefusal::ModeNotSupported)
                }
            };
            if let Some(reason) = reason {
                let r = refused
                    .get_mut(refused_len)
                    .ok_or(ObserveError::TooManyRefused)?;
                *r = ObserveRefused {
                    index: entry.index,
                    reason,
                };
                refused_len += 1;
            }
        }

        // second pass: the first entry of each index wins, as checked above
        let mut modes = [Mode::Off; N];
        if req.patch {
            for (m, s) in modes.iter_mut().zip(self.slots.iter()) {
                *m = s.mode;
            }
        }
        let mut applied = [false; N];
        for entry in req.entries.iter().flatten() {
            let Some(k) = desc.slot_of(entry.index.0) else {
                continue;
            };
            if applied[k] {
                continue;
            }
            applied[k] = true;
            if let Some(mode) = Mode::clamped(&entry.mode, limits, desc.batched[k]) {
                modes[k] = mode;
            }
        }
        // one row per sample of the level, one time column per update: its `Samples` slots share the finest
        // `every` and the shortest `max_age` asked for
        let shared = modes
            .iter()
            .fold(None, |shared, mode| match (*mode, shared) {
                (Mode::Samples { every, max_age }, None) => Some((every, max_age)),
                (Mode::Samples { every, max_age }, Some((e, a))) => {
                    Some((every.min(e), max_age.min(a)))
                }
                (_, shared) => shared,
            });
        if let Some((every, max_age)) = shared {
            for mode in modes.iter_mut() {
                if let Mode::Samples { .. } = mode {
                    *mode = Mode::Samples { every, max_age };
                }
            }
        }
        let was_active = self.is_active();
        for ((slot, mode), listed) in self.slots.iter_mut().zip(modes).zip(applied) {
            // a listed property starts over: sent once right away, so a host asking again gets fresh values
            if slot.mode != mode || listed {
                let dirty = slot.dirty;
                *slot = Slot {
                    mode,
                    dirty,
                    start: mode != Mode::Off,
                    ..Slot::default()
                };
            }
        }
        if self.is_active() {
            let wanted = req.budget.map_or(limits.default_budget, |b| b.0);
            self.budget = wanted
                .min(limits.max_level_budget)
                .min(limits.total_budget.saturating_sub(others_budget));
            if !was_active {
                self.tokens = Self::capacity(self.budget);
                self.last_refill = None;
                self.missed = 0;
            }
            self.tokens = self.tokens.min(Self::capacity(self.budget));
        } else {
            self.budget = 0;
        }
        Ok(refused_len)
    }

    /// The settings as applied, for the answer to `Observe`: every observed property in index order. Returns how
    /// many entries of `out` are filled.
    pub fn granted(&self, desc: &LevelDesc<N>, out: &mut [ObserveEntry; N]) -> usize {
        let mut len = 0;
        for (slot, index) in self.slots.iter().zip(desc.observable) {
            if slot.mode != Mode::Off {
                out[len] = ObserveEntry {
                    index: UNib32(index),
                    mode: slot.mode.to_wire(),
                };
                len += 1;
            }
        }
        len
    }

    /// `Samples` mode of slot `k` (API-39): `(every_us, max_age_us)`, None in any other mode.
    pub fn samples(&self, k: usize) -> Option<(u32, u32)> {
        match self.slots.get(k)?.mode {
            Mode::Samples { every, max_age } => Some((every, max_age)),
            _ => None,
        }
    }

    /// `Samples` mode of the level (API-39): `(every_us, max_age_us)`, the same for every slot in that mode, None
    /// when no slot is in it.
    pub fn level_samples(&self) -> Option<(u32, u32)> {
        (0..N).find_map(|k| self.samples(k))
    }

    /// Samples lost on the device (dropped from a full outbox, or the event loop was behind the sampler): reported
    /// as missed in the next update.
    pub fn add_missed(&mut self, samples: u64) {
        let samples = u32::try_from(samples).unwrap_or(u32::MAX);
        self.missed = self.missed.saturating_add(samples);
    }

    fn capacity(budget: u32) -> i64 {
        (budget / 10).max(MIN_BURST) as i64 * SCALE
    }

    fn refill(&mut self, now_us: u64) {
        let elapsed = match self.last_refill {
            Some(last) => now_us.saturating_sub(last),
            None => 0,
        };
        if self.last_refill.is_none_or(|last| now_us > last) {
            self.last_refill = Some(now_us);
        }
        let elapsed = i64::try_from(elapsed).unwrap_or(i64::MAX);
        let add = elapsed.saturating_mul(self.budget as i64);
        self.tokens = self
            .tokens
            .saturating_add(add)
            .min(Self::capacity(self.budget));
    }

    /// What is due at `now_us` (any microsecond clock), earliest deadline first. Refills the bucket and counts
    /// the periods periodic properties fell behind. Without properties in `Samples` mode, see [plan_with].
    ///
    /// [plan_with]: Self::plan_with
    pub fn plan(&mut self, now_us: u64, limits: &ObserveLimits) -> Plan<N> {
        self.plan_with(now_us, limits, |_| false)
    }

    /// [plan](Self::plan), where a slot in `Samples` mode is due when `batch_waiting(k)` (its outbox holds a batch
    /// of it), from when it was first seen waiting.
    pub fn plan_with(
        &mut self,
        now_us: u64,
        limits: &ObserveLimits,
        batch_waiting: impl Fn(usize) -> bool,
    ) -> Plan<N> {
        let mut plan = Plan {
            order: [0; N],
            deadline: [0; N],
            len: 0,
            tokens: 0,
            full: false,
            missed: self.missed,
            t: limits.timestamps.then_some(now_us),
        };
        if !self.is_active() {
            return plan;
        }
        self.refill(now_us);
        plan.tokens = (self.tokens / SCALE).max(0) as usize;
        plan.full = self.tokens >= Self::capacity(self.budget);
        for (k, slot) in self.slots.iter_mut().enumerate() {
            if slot.start {
                slot.start = false;
                slot.dirty = true;
                slot.next_due = now_us;
                slot.last_sent = None;
            }
            let due = match slot.mode {
                Mode::Off => None,
                Mode::Periodic { every } => {
                    let every = every as u64;
                    if now_us >= slot.next_due.saturating_add(every) {
                        // whole periods behind: those sends are lost, the one counted at its slip already
                        let behind = (now_us - slot.next_due) / every;
                        let lost = behind.saturating_sub(slot.slipped as u64);
                        self.missed = self.missed.saturating_add(lost.min(u32::MAX as u64) as u32);
                        slot.next_due = slot.next_due.saturating_add(behind * every);
                        slot.slipped = false;
                    }
                    (now_us >= slot.next_due).then_some(slot.next_due)
                }
                Mode::OnChange { min } => match slot.last_sent {
                    _ if !slot.dirty => None,
                    // the initial value: due since observing started
                    None => Some(slot.next_due),
                    Some(last) => {
                        let at = last.saturating_add(min as u64);
                        (now_us >= at).then_some(at)
                    }
                },
                Mode::Samples { .. } => batch_waiting(k).then_some(now_us),
                Mode::OnChangeOrBeat { min, max } => match slot.last_sent {
                    None => Some(slot.next_due),
                    Some(last) => {
                        let at_min = last.saturating_add(min as u64);
                        let at_max = last.saturating_add(max as u64);
                        if slot.dirty && now_us >= at_min {
                            Some(at_min)
                        } else if now_us >= at_max {
                            Some(at_max)
                        } else {
                            None
                        }
                    }
                },
            };
            if let Some(deadline) = due {
                let deadline = slot.waiting.map_or(deadline, |w| w.min(deadline));
                // insertion by deadline, ties in slot order
                let mut i = plan.len;
                while i > 0 && plan.deadline[i - 1] > deadline {
                    plan.order[i] = plan.order[i - 1];
                    plan.deadline[i] = plan.deadline[i - 1];
                    i -= 1;
                }
                plan.order[i] = k;
                plan.deadline[i] = deadline;
                plan.len += 1;
            }
        }
        // periods counted behind just now go with this update
        plan.missed = self.missed;
        plan
    }

    /// After an update for `plan` with `sel`: `sent` is the length of the event as sent, None when it could not
    /// be sent (the chosen properties count as missed then). Moves the chosen and dropped properties on and
    /// counts the ones that slipped.
    pub fn commit(&mut self, plan: &Plan<N>, sel: &Selection<N>, now_us: u64, sent: Option<usize>) {
        match sent {
            Some(len) => {
                self.tokens = self.tokens.saturating_sub(len as i64 * SCALE);
                self.missed = self.missed.saturating_sub(plan.missed);
            }
            None => {
                let chosen = sel.chosen.iter().filter(|c| **c).count() as u32;
                self.missed = self.missed.saturating_add(chosen);
            }
        }
        for (&k, &deadline) in plan.edf().iter().zip(&plan.deadline) {
            let slot = &mut self.slots[k];
            if sel.chosen[k] || sel.dropped[k] {
                if sel.dropped[k] {
                    self.missed = self.missed.saturating_add(1);
                }
                slot.dirty = false;
                slot.slipped = false;
                slot.waiting = None;
                slot.last_sent = Some(now_us);
                if let Mode::Periodic { every } = slot.mode {
                    slot.next_due = slot.next_due.saturating_add(every as u64);
                }
            } else {
                slot.waiting.get_or_insert(deadline);
                // late by its `min` or more: one missed send (a periodic one counts in `plan`, per whole period)
                // samples left out wait in the outbox, which counts what it drops
                let grace = match slot.mode {
                    Mode::OnChange { min } | Mode::OnChangeOrBeat { min, .. } => Some(min as u64),
                    Mode::Periodic { .. } | Mode::Samples { .. } | Mode::Off => None,
                };
                let late = now_us.saturating_sub(slot.waiting.unwrap_or(deadline));
                if !slot.slipped && grace.is_some_and(|g| late >= g) {
                    slot.slipped = true;
                    self.missed = self.missed.saturating_add(1);
                }
            }
        }
    }
}

/// What a level has to send at one poll, see [ObserveTable::plan].
#[derive(Debug, Clone)]
pub struct Plan<const N: usize> {
    order: [usize; N],
    deadline: [u64; N],
    len: usize,
    /// Whole bytes in the bucket.
    tokens: usize,
    /// The bucket is full.
    full: bool,
    missed: u32,
    t: Option<u64>,
}

impl<const N: usize> Plan<N> {
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Slots of the due properties, earliest deadline first.
    pub fn edf(&self) -> &[usize] {
        &self.order[..self.len]
    }

    /// Nothing is due, but sends or samples were missed: an update with no properties carries the count alone
    /// (`overhead` as for [selection](Self::selection)) when the budget has room for it. A count that waited for
    /// the next update would never reach the host when nothing else comes, as after the last samples of a burst
    /// the sampler lost.
    pub fn report_missed(&self, desc: &LevelDesc<N>, overhead: usize) -> bool {
        self.len == 0
            && self.missed > 0
            && self.tokens >= overhead + EVENT_SLACK + desc.header_bound()
    }

    /// `missed` of the update: sends missed before this poll, None when there were none.
    pub fn missed(&self) -> Option<UNib32> {
        (self.missed > 0).then_some(UNib32(self.missed))
    }

    /// `t` of the update, with [ObserveLimits::timestamps]: microseconds as a wrapping `u32`, as stream heads have
    /// it.
    pub fn t(&self) -> Option<ww_si::Second> {
        self.t.map(|us| {
            ww_si::Second::from_parts(
                ww_si::Prefix::Micro,
                ww_si::ww_numeric::NumericValue::U32(us as u32),
            )
        })
    }

    /// Start choosing what goes into the update: `overhead` is the length of the event without data (see
    /// [event_overhead]), `buf_len` the most the data may take.
    pub fn selection(&self, desc: &LevelDesc<N>, overhead: usize, buf_len: usize) -> Selection<N> {
        let fixed = overhead + EVENT_SLACK + desc.header_bound();
        Selection {
            chosen: [false; N],
            dropped: [false; N],
            count: 0,
            est: fixed,
            limit: self.tokens,
            max_len: buf_len.saturating_add(overhead),
            fixed,
            full: self.full,
            held: false,
            done: false,
            time: false,
        }
    }
}

/// The properties chosen for one update, offered in [Plan::edf] order with their estimated size.
#[derive(Debug, Clone)]
pub struct Selection<const N: usize> {
    chosen: [bool; N],
    /// Can't be sent this time (value not readable, bigger than a message), moved on as missed.
    dropped: [bool; N],
    count: usize,
    est: usize,
    /// Bytes in the bucket.
    limit: usize,
    /// Most an event may take.
    max_len: usize,
    /// Estimate of an update without values.
    fixed: usize,
    full: bool,
    held: bool,
    done: bool,
    /// The level's `sample_times` slot goes into the update (a batch of samples does).
    time: bool,
}

impl<const N: usize> Selection<N> {
    /// Offer slot `k` with the upper bound of its value's size, None when the value can't be had. Returns false
    /// once the update is full: offer nothing more.
    pub fn offer(&mut self, k: usize, size: Option<usize>) -> bool {
        if k >= N {
            self.done = true;
            return false;
        }
        self.offer_all(&[k], size, false)
    }

    /// Offer a batch of samples (API-39): the slots `ks` of its columns and the level's `sample_times` slot, all or
    /// none, `size` the upper bound of them all. Returns false once the update is full: offer nothing more.
    pub fn offer_batch(&mut self, ks: &[usize], size: usize) -> bool {
        if ks.iter().any(|&k| k >= N) {
            self.done = true;
            return false;
        }
        self.offer_all(ks, Some(size), true)
    }

    fn offer_all(&mut self, ks: &[usize], size: Option<usize>, time: bool) -> bool {
        if self.done {
            return false;
        }
        let Some(size) = size else {
            for &k in ks {
                self.dropped[k] = true;
            }
            return true;
        };
        let est = self.est.saturating_add(size);
        if est <= self.limit && est <= self.max_len {
            self.choose(ks, est, time);
            true
        } else if self.fixed.saturating_add(size) > self.max_len {
            // never fits a message
            for &k in ks {
                self.dropped[k] = true;
            }
            true
        } else if self.count == 0 && self.full {
            // bigger than the bucket: alone, when the bucket is full
            self.choose(ks, est, time);
            self.done = true;
            false
        } else if !self.full && est <= self.max_len {
            // the bucket is what limits and it isn't full: hold, send nothing, rather than part of what is due
            self.chosen = [false; N];
            self.count = 0;
            self.time = false;
            self.held = true;
            self.done = true;
            false
        } else {
            self.done = true;
            false
        }
    }

    fn choose(&mut self, ks: &[usize], est: usize, time: bool) {
        for &k in ks {
            self.chosen[k] = true;
        }
        self.count += ks.len();
        self.est = est;
        self.time |= time;
    }

    /// Whether the level's `sample_times` slot goes into the update.
    pub fn has_time(&self) -> bool {
        self.time
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Nothing goes out this poll: the level waits for its bucket to hold everything due.
    pub fn is_held(&self) -> bool {
        self.held
    }

    /// Whether slot `k` goes into the update.
    pub fn is_chosen(&self, k: usize) -> bool {
        self.chosen.get(k).copied().unwrap_or(false)
    }

    /// Whether slot `k` can't be sent this time (its value can't be had, or it never fits a message): moved on as
    /// missed.
    pub fn is_dropped(&self, k: usize) -> bool {
        self.dropped.get(k).copied().unwrap_or(false)
    }

    /// Slot `k` could not be written after all: moved on as missed.
    pub fn drop_slot(&mut self, k: usize) {
        if k < N {
            self.chosen[k] = false;
            self.dropped[k] = true;
        }
    }

    /// Resource indices of the chosen properties, increasing, and the level's `sample_times` slot last when a batch
    /// of samples goes: the present slots of the update's `OptionAnyVec`.
    pub fn present<'a>(&'a self, desc: &'a LevelDesc<N>) -> impl Iterator<Item = u32> + Clone + 'a {
        desc.observable
            .iter()
            .zip(self.chosen.iter())
            .filter(|(_, chosen)| **chosen)
            .map(|(index, _)| *index)
            .chain(desc.time_slot.filter(|_| self.time))
    }
}

/// Most bytes one level batch of samples (API-39) may take in an update of a level, its time column and `columns`
/// value columns together, so that it fits `buf_len` bytes of data next to an `overhead` ([event_overhead]): the
/// update's presence header and each slot's length and `Batched` tag left out. The batcher seals at it; an outbox of
/// `outbox_bytes` can hold no more than what is left of it after the entries' headers.
pub fn batch_budget<const N: usize>(
    desc: &LevelDesc<N>,
    overhead: usize,
    buf_len: usize,
    outbox_bytes: usize,
    columns: usize,
) -> usize {
    let slots = columns + 1;
    let fixed = overhead + EVENT_SLACK + desc.header_bound() + slots * (LEN_SLOT + 1);
    let outbox_room = outbox_bytes.saturating_sub(slots * crate::batch::OUTBOX_ENTRY_HEADER);
    buf_len
        .saturating_add(overhead)
        .saturating_sub(fixed)
        .min(outbox_room)
}

/// Most updates of one level a call of `poll_properties` sends while batches of samples wait (API-39); the rest
/// waits for the next call.
pub const MAX_UPDATES_PER_POLL: usize = 32;

/// The device time of samples, re-exported for generated code.
pub use ww_si::Second;

/// [event_overhead] of the level at `path` with the longest `t` and `missed` an update can have: what
/// [batch_budget] is worked out from, before the update's own head is known.
pub fn batch_overhead(buf: &mut [u8], path: &[UNib32]) -> Result<usize, ShrinkWrapError> {
    let t = ww_si::Second::from_parts(
        ww_si::Prefix::Micro,
        ww_si::ww_numeric::NumericValue::U32(u32::MAX),
    );
    event_overhead(buf, path, Some(t), Some(UNib32(u32::MAX)))
}

/// Upper bound of the bytes a `Batched::One` of a `T` takes in an update, when `T` has one: the tag, the value,
/// the slot's length.
pub const fn max_batched_one_len<T: SerializeShrinkWrap>() -> Option<usize> {
    match max_write_len::<T>() {
        Some(len) => Some(len + 1 + LEN_SLOT),
        None => None,
    }
}

/// Bytes an encoded batch of `len` bytes takes as a slot of an update: the `Batched` tag and padding, the length.
pub const fn batch_slot_len(len: usize) -> usize {
    len + 1 + LEN_SLOT
}

/// Which samples of one `+batched` property its sampler takes (API-39): none, all, or the first one in each
/// `every_us`, as the event loop sets it from the host's mode. The sampler reads it with one atomic load per
/// sample; a [GateCursor] in the sampler keeps the grid.
#[derive(Debug)]
pub struct SampleGate {
    /// µs between samples taken, [GATE_OFF]: none
    every: AtomicU32,
    /// Bumped by every [set](Self::set): the sampler starts its grid over
    generation: AtomicU32,
}

const GATE_OFF: u32 = u32::MAX;

impl SampleGate {
    /// Takes no samples.
    pub const fn new() -> Self {
        SampleGate {
            every: AtomicU32::new(GATE_OFF),
            generation: AtomicU32::new(0),
        }
    }

    /// From the property's mode: `Some(every_us)` in `Samples` mode (0: every sample), None otherwise. Changes
    /// nothing when it is set already.
    pub fn set(&self, every_us: Option<u32>) {
        let every = every_us.map_or(GATE_OFF, |e| e.min(GATE_OFF - 1));
        if self.every.load(Ordering::Relaxed) != every {
            self.generation.fetch_add(1, Ordering::Relaxed);
            self.every.store(every, Ordering::Release);
        }
    }

    /// Whether samples are taken at all.
    pub fn is_open(&self) -> bool {
        self.every.load(Ordering::Relaxed) != GATE_OFF
    }
}

impl Default for SampleGate {
    fn default() -> Self {
        Self::new()
    }
}

/// The sampler's side of a [SampleGate]: where its grid is.
#[derive(Debug, Clone, Copy, Default)]
pub struct GateCursor {
    generation: u32,
    /// µs (wrapping) of the next sample to take on the grid, None: the next one
    next: Option<u32>,
}

impl GateCursor {
    pub const fn new() -> Self {
        GateCursor {
            generation: 0,
            next: None,
        }
    }

    /// Whether to take the sample at `t` (device time), O(1): `t` is read as microseconds only when the gate
    /// decimates. A time that can't be read as microseconds (a float, a huge value) is taken.
    pub fn take(&mut self, gate: &SampleGate, t: &ww_si::Second) -> bool {
        let every = gate.every.load(Ordering::Acquire);
        if every == GATE_OFF {
            return false;
        }
        let generation = gate.generation.load(Ordering::Relaxed);
        if generation != self.generation {
            self.generation = generation;
            self.next = None;
        }
        if every == 0 {
            return true;
        }
        let Some(us) = t.to_i128_in(ww_si::Prefix::Micro) else {
            return true;
        };
        // wrapping microseconds: the device clock and the grid wrap together
        let us = us as u32;
        if let Some(next) = self.next {
            // before the next point of the grid, by at most a period
            let ahead = next.wrapping_sub(us);
            if ahead != 0 && ahead <= every {
                return false;
            }
            // at or past it: on to the following one, or, behind by a period or more (or the clock jumped), the
            // grid starts over from this sample
            let behind = us.wrapping_sub(next);
            self.next = Some(if behind < every {
                next.wrapping_add(every)
            } else {
                us.wrapping_add(every)
            });
        } else {
            self.next = Some(us.wrapping_add(every));
        }
        true
    }
}

/// Upper bound of the bytes a value of `T` takes in an update, when the type has one.
pub const fn max_write_len<T: SerializeShrinkWrap + ?Sized>() -> Option<usize> {
    wire_weaver::shrink_wrap::MaxSize::buffer_len_of(wire_weaver::shrink_wrap::write_max_size::<T>())
}

/// Upper bound of the bytes `value` takes in an update, serialized into `buf` to find out; None when it doesn't
/// fit.
pub fn measure<T: SerializeShrinkWrap + ?Sized>(value: &T, buf: &mut [u8]) -> Option<usize> {
    if let Some(len) = max_write_len::<T>() {
        return Some(len);
    }
    let len = value.to_ww_bytes(buf).ok()?.len();
    Some(len + LEN_SLOT)
}

/// Length of the `PropertyUpdate` event of the level at `path` with no data, serialized into `buf`.
pub fn event_overhead(
    buf: &mut [u8],
    path: &[UNib32],
    t: Option<ww_si::Second>,
    missed: Option<UNib32>,
) -> Result<usize, ShrinkWrapError> {
    ww_client_server::observe::ser_property_update_event(buf, path, t, missed, |_| Ok(0))
        .map(|event| event.len())
}

#[cfg(test)]
mod tests;

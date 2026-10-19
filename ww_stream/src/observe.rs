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
use wire_weaver::shrink_wrap::option_any_vec::OptionAnyVecWriter;
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
pub struct LevelDesc<'a> {
    /// Number of resource indices of the level (largest one + 1), properties or not.
    pub item_count: u32,
    pub observable: &'a [u32],
    /// `+batched` (API-39): the slot takes `Samples` mode, its value is written as a `Batched`.
    pub batched: &'a [bool],
}

impl LevelDesc<'_> {
    fn slot_of(&self, index: u32) -> Option<usize> {
        self.observable.binary_search(&index).ok()
    }

    fn is_batched(&self, k: usize) -> bool {
        self.batched.get(k).copied().unwrap_or(false)
    }

    /// Upper bound of the presence header of an update: never longer than the dense mode's, a mode nibble,
    /// the `UNib32` slot count and a bit per slot.
    fn header_bound(&self) -> usize {
        let len = self.observable.last().map_or(0, |last| last + 1);
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

/// Settings and send state of one observable property in an [ObserveTable].
#[derive(Debug, Clone, Copy)]
pub struct Slot {
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
    /// Seen while [ObserveTable::apply] goes through a request, false otherwise.
    mark: bool,
}

impl Slot {
    /// Not observed.
    pub const OFF: Slot = Slot {
        mode: Mode::Off,
        next_due: 0,
        last_sent: None,
        dirty: false,
        slipped: false,
        waiting: None,
        start: false,
        mark: false,
    };

    /// Observed in `mode` from now on, sent once right away; a change waiting to be sent stays.
    fn restart(&mut self, mode: Mode) {
        *self = Slot {
            mode,
            dirty: self.dirty,
            start: mode != Mode::Off,
            ..Slot::OFF
        };
    }
}

impl Default for Slot {
    fn default() -> Self {
        Slot::OFF
    }
}

/// Why an `Observe` request was refused as a whole, nothing applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObserveError {
    /// An entry could not be read.
    Malformed(ShrinkWrapError),
    /// More entries refused than the answer has room for.
    TooManyRefused,
}

/// Observe settings and send state of one trait level: a [Slot] per observable property.
///
/// A level keeps an `ObserveTable<[Slot; N]>`, sized at compile time; the methods are on `ObserveTable<[Slot]>`,
/// the same code for every level, reached through `Deref` (or the coercion of `&mut ObserveTable<[Slot; N]>` to
/// `&mut ObserveTable`).
#[derive(Debug, Clone)]
pub struct ObserveTable<S: ?Sized = [Slot]> {
    /// Granted bytes per second, 0 while nothing is observed.
    budget: u32,
    /// Byte-microseconds in the bucket.
    tokens: i64,
    last_refill: Option<u64>,
    /// Missed sends not reported to the host yet.
    missed: u32,
    /// Bytes the level waits for in its bucket (SRV-25): what its last update held back for (`Selection` held), or
    /// a missed count waiting alone; 0 when it waits for nothing.
    need: u32,
    slots: S,
}

impl<const N: usize> Default for ObserveTable<[Slot; N]> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> ObserveTable<[Slot; N]> {
    /// Nothing observed.
    pub const fn new() -> Self {
        ObserveTable {
            budget: 0,
            tokens: 0,
            last_refill: None,
            missed: 0,
            need: 0,
            slots: [Slot::OFF; N],
        }
    }

    /// [new](Self::new) in place, slot by slot: for an `ObserveState` built in a `static`
    /// (`ObserveState::init`), nothing of the table on the stack on the way.
    pub fn init(slot: &mut core::mem::MaybeUninit<Self>) -> &mut Self {
        // every field is written below: a new one fails to compile here until it is
        let _ = |t: Self| {
            let ObserveTable {
                budget: _,
                tokens: _,
                last_refill: _,
                missed: _,
                need: _,
                slots: _,
            } = t;
        };
        let at = slot.as_mut_ptr();
        // SAFETY: every field written in place, each through its own pointer; `[Slot; N]` is `N` slots one after
        // the other
        unsafe {
            let slots = core::ptr::addr_of_mut!((*at).slots).cast::<Slot>();
            for k in 0..N {
                slots.add(k).write(Slot::OFF);
            }
            core::ptr::addr_of_mut!((*at).budget).write(0);
            core::ptr::addr_of_mut!((*at).tokens).write(0);
            core::ptr::addr_of_mut!((*at).last_refill).write(None);
            core::ptr::addr_of_mut!((*at).missed).write(0);
            core::ptr::addr_of_mut!((*at).need).write(0);
            slot.assume_init_mut()
        }
    }
}

impl<const N: usize> core::ops::Deref for ObserveTable<[Slot; N]> {
    type Target = ObserveTable;

    fn deref(&self) -> &ObserveTable {
        self
    }
}

impl<const N: usize> core::ops::DerefMut for ObserveTable<[Slot; N]> {
    fn deref_mut(&mut self) -> &mut ObserveTable {
        self
    }
}

impl ObserveTable {
    /// Stop observing everything, e.g., when the host is gone.
    pub fn reset(&mut self) {
        self.slots.fill(Slot::OFF);
        self.budget = 0;
        self.tokens = 0;
        self.last_refill = None;
        self.missed = 0;
        self.need = 0;
    }

    /// Number of observable properties of the level.
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// Whether the level has no observable properties.
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
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
    /// returned. `desc` describes the same slots as the table.
    pub fn apply(
        &mut self,
        req: &ObserveRequest<'_>,
        desc: &LevelDesc<'_>,
        limits: &ObserveLimits,
        others_budget: u32,
        refused: &mut [ObserveRefused],
    ) -> Result<usize, ObserveError> {
        debug_assert_eq!(self.slots.len(), desc.observable.len());
        // first pass: check everything, change nothing but the marks
        let checked = self.check(req, desc, limits, refused);
        for slot in self.slots.iter_mut() {
            slot.mark = false;
        }
        let refused_len = checked?;

        // second pass: the first entry of each index wins, as checked above; a listed property starts over, sent
        // once right away, so a host asking again gets fresh values
        let was_active = self.is_active();
        for entry in req.entries.iter().flatten() {
            let Some(k) = desc.slot_of(entry.index.0) else {
                continue;
            };
            let Some(slot) = self.slots.get_mut(k) else {
                continue;
            };
            if slot.mark {
                continue;
            }
            let unlisted = if req.patch { slot.mode } else { Mode::Off };
            slot.restart(
                Mode::clamped(&entry.mode, limits, desc.is_batched(k)).unwrap_or(unlisted),
            );
            slot.mark = true;
        }
        for slot in self.slots.iter_mut() {
            let listed = core::mem::take(&mut slot.mark);
            if !listed && !req.patch && slot.mode != Mode::Off {
                slot.restart(Mode::Off);
            }
        }
        // one row per sample of the level, one time column per update: its `Samples` slots share the finest
        // `every` and the shortest `max_age` asked for
        let shared = self
            .slots
            .iter()
            .fold(None, |shared, slot| match (slot.mode, shared) {
                (Mode::Samples { every, max_age }, None) => Some((every, max_age)),
                (Mode::Samples { every, max_age }, Some((e, a))) => {
                    Some((every.min(e), max_age.min(a)))
                }
                (_, shared) => shared,
            });
        if let Some((every, max_age)) = shared {
            let mode = Mode::Samples { every, max_age };
            for slot in self.slots.iter_mut() {
                if matches!(slot.mode, Mode::Samples { .. }) && slot.mode != mode {
                    slot.restart(mode);
                }
            }
        }
        if self.is_active() {
            let wanted = req.budget.map_or(limits.default_budget, |b| b.0);
            self.budget = wanted
                .min(limits.max_level_budget)
                .min(limits.total_budget.saturating_sub(others_budget));
            if !was_active {
                self.tokens = capacity(self.budget);
                self.last_refill = None;
                self.missed = 0;
            }
            self.tokens = self.tokens.min(capacity(self.budget));
        } else {
            self.budget = 0;
        }
        // what was held back may not be due any more, a new budget fills at another rate
        self.need = 0;
        Ok(refused_len)
    }

    /// The first pass of [apply](Self::apply): every entry read and checked, refused ones put into `refused`; the
    /// slots seen are marked.
    fn check(
        &mut self,
        req: &ObserveRequest<'_>,
        desc: &LevelDesc<'_>,
        limits: &ObserveLimits,
        refused: &mut [ObserveRefused],
    ) -> Result<usize, ObserveError> {
        let mut refused_len = 0;
        for entry in req.entries.iter() {
            let entry = entry.map_err(ObserveError::Malformed)?;
            let reason = match desc.slot_of(entry.index.0) {
                None if entry.index.0 >= desc.item_count => Some(ObserveRefusal::NoSuchIndex),
                None => Some(ObserveRefusal::NotObservable),
                Some(k) => match self.slots.get_mut(k) {
                    None => Some(ObserveRefusal::NotObservable),
                    Some(slot) if slot.mark => Some(ObserveRefusal::Duplicate),
                    Some(slot) => {
                        slot.mark = true;
                        Mode::clamped(&entry.mode, limits, desc.is_batched(k))
                            .is_none()
                            .then_some(ObserveRefusal::ModeNotSupported)
                    }
                },
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
        Ok(refused_len)
    }

    /// The settings as applied, for the answer to `Observe`: every observed property in index order, as many as
    /// `out` (one entry per slot) has room for. Returns how many entries of `out` are filled.
    pub fn granted(&self, desc: &LevelDesc<'_>, out: &mut [ObserveEntry]) -> usize {
        let mut len = 0;
        for (slot, &index) in self.slots.iter().zip(desc.observable) {
            if slot.mode == Mode::Off {
                continue;
            }
            let Some(entry) = out.get_mut(len) else {
                break;
            };
            *entry = ObserveEntry {
                index: UNib32(index),
                mode: slot.mode.to_wire(),
            };
            len += 1;
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
        self.slots.iter().find_map(|slot| match slot.mode {
            Mode::Samples { every, max_age } => Some((every, max_age)),
            _ => None,
        })
    }

    /// Samples lost on the device (dropped from a full outbox, or the event loop was behind the sampler): reported
    /// as missed in the next update.
    pub fn add_missed(&mut self, samples: u64) {
        let samples = u32::try_from(samples).unwrap_or(u32::MAX);
        self.missed = self.missed.saturating_add(samples);
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
        self.tokens = self.tokens.saturating_add(add).min(capacity(self.budget));
    }

    /// What is due at `now_us` (any microsecond clock), earliest deadline first, written into `due` (one entry per
    /// slot). Refills the bucket and counts the periods periodic properties fell behind. Without properties in
    /// `Samples` mode, see [plan_with].
    ///
    /// [plan_with]: Self::plan_with
    pub fn plan<'a>(
        &mut self,
        now_us: u64,
        limits: &ObserveLimits,
        due: &'a mut [Due],
    ) -> Plan<'a> {
        self.plan_with(now_us, limits, &|_| false, due)
    }

    /// [plan](Self::plan), where a slot in `Samples` mode is due when `batch_waiting(k)` (its outbox holds a batch
    /// of it), from when it was first seen waiting.
    pub fn plan_with<'a>(
        &mut self,
        now_us: u64,
        limits: &ObserveLimits,
        batch_waiting: &dyn Fn(usize) -> bool,
        due: &'a mut [Due],
    ) -> Plan<'a> {
        debug_assert!(due.len() >= self.slots.len());
        let mut plan = Plan {
            due: &[],
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
        plan.full = self.tokens >= capacity(self.budget);
        let mut len = 0;
        for (k, slot) in self.slots.iter_mut().enumerate().take(due.len()) {
            if slot.start {
                slot.start = false;
                slot.dirty = true;
                slot.next_due = now_us;
                slot.last_sent = None;
            }
            let deadline = match slot.mode {
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
            if let Some(deadline) = deadline {
                let deadline = slot.waiting.map_or(deadline, |w| w.min(deadline));
                // insertion by deadline, ties in slot order
                let mut i = len;
                while i > 0 && due[i - 1].deadline > deadline {
                    due[i] = due[i - 1];
                    i -= 1;
                }
                due[i] = Due {
                    k,
                    deadline,
                    samples: matches!(slot.mode, Mode::Samples { .. }),
                };
                len += 1;
            }
        }
        // periods counted behind just now go with this update
        plan.missed = self.missed;
        let due: &'a [Due] = due;
        plan.due = &due[..len];
        plan
    }

    /// When the level next has something to do (SRV-25), given that nothing changes before: the earliest time a
    /// property becomes due (the periodic grid, `last + min` of a changed one, `last + max` of a beat, at once for
    /// one just observed, left out, or whose batch waits, `batch_waiting(k)` as for [plan_with]), and no earlier
    /// than its bucket has the bytes the level waits for ([hold](Self::hold), or what the last update held back
    /// for). `None` while nothing is observed or nothing comes due without a change. A time at or before `now_us`
    /// means now. A change ([mark_changed](Self::mark_changed), `ObserveSignal`) or a new batch can make it
    /// earlier: ask again after one.
    ///
    /// [plan_with]: Self::plan_with
    pub fn next_wake(&self, now_us: u64, batch_waiting: &dyn Fn(usize) -> bool) -> Option<u64> {
        if !self.is_active() {
            return None;
        }
        let mut due: Option<u64> = (self.missed > 0).then_some(now_us);
        for (k, slot) in self.slots.iter().enumerate() {
            let at = if slot.start || slot.waiting.is_some() {
                Some(now_us)
            } else {
                match slot.mode {
                    Mode::Off => None,
                    Mode::Periodic { .. } => Some(slot.next_due),
                    Mode::OnChange { min } => match slot.last_sent {
                        _ if !slot.dirty => None,
                        None => Some(now_us),
                        Some(last) => Some(last.saturating_add(min as u64)),
                    },
                    Mode::OnChangeOrBeat { min, max } => match slot.last_sent {
                        None => Some(now_us),
                        Some(last) if slot.dirty => Some(last.saturating_add(min as u64)),
                        Some(last) => Some(last.saturating_add(max as u64)),
                    },
                    Mode::Samples { .. } => batch_waiting(k).then_some(now_us),
                }
            };
            due = match (due, at) {
                (Some(d), Some(a)) => Some(d.min(a)),
                (d, a) => d.or(a),
            };
        }
        let due = due?;
        if self.need == 0 {
            return Some(due);
        }
        // byte-microseconds still missing, filled at `budget` per microsecond since the last refill
        let want = (self.need as i64)
            .saturating_mul(SCALE)
            .min(capacity(self.budget));
        let missing = want.saturating_sub(self.tokens);
        if missing <= 0 {
            return Some(due);
        }
        if self.budget == 0 {
            return None;
        }
        let since = self.last_refill.unwrap_or(now_us);
        let refilled = since.saturating_add((missing as u64).div_ceil(self.budget as u64));
        Some(due.max(refilled))
    }

    /// The level waits for `bytes` in its bucket before it sends (SRV-25): a missed count that goes alone, see
    /// [Plan::missed_need]. [commit](Self::commit) sets it from the update's selection.
    pub fn hold(&mut self, bytes: usize) {
        self.need = bytes.min(u32::MAX as usize) as u32;
    }

    /// After an update for `plan` with `sel`: `sent` is the length of the event as sent, None when it could not
    /// be sent (the chosen properties count as missed then). Moves the chosen and dropped properties on and
    /// counts the ones that slipped.
    pub fn commit(
        &mut self,
        plan: &Plan<'_>,
        sel: &Selection<'_>,
        now_us: u64,
        sent: Option<usize>,
    ) {
        self.need = sel.need.min(u32::MAX as usize) as u32;
        match sent {
            Some(len) => {
                self.tokens = self.tokens.saturating_sub(len as i64 * SCALE);
                self.missed = self.missed.saturating_sub(plan.missed);
            }
            None => {
                let chosen = sel.marks.iter().filter(|m| m.is_chosen()).count() as u32;
                self.missed = self.missed.saturating_add(chosen);
            }
        }
        for &Due { k, deadline, .. } in plan.due {
            let Some(slot) = self.slots.get_mut(k) else {
                continue;
            };
            let mark = sel.marks.get(k).copied().unwrap_or(Mark::NONE);
            if mark.is_chosen() || mark.is_dropped() {
                if mark.is_dropped() {
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

fn capacity(budget: u32) -> i64 {
    (budget / 10).max(MIN_BURST) as i64 * SCALE
}

/// Answer an `Observe` request for a level without observable properties: every entry refused (`NoSuchIndex`
/// beyond `item_count`, else `NotObservable`), into `refused`; returns their count. Nothing is granted.
pub fn refuse_all(
    req: &ObserveRequest<'_>,
    item_count: u32,
    refused: &mut [ObserveRefused],
) -> Result<usize, ObserveError> {
    let mut table = ObserveTable::<[Slot; 0]>::new();
    let desc = LevelDesc {
        item_count,
        observable: &[],
        batched: &[],
    };
    table.apply(req, &desc, &ObserveLimits::DEFAULT, 0, refused)
}

/// A property due at one poll, see [ObserveTable::plan]: storage for the plan, one per slot of the level.
#[derive(Debug, Clone, Copy, Default)]
pub struct Due {
    k: usize,
    deadline: u64,
    /// In `Samples` mode: due with the level's batch of samples.
    samples: bool,
}

impl Due {
    pub const EMPTY: Due = Due {
        k: 0,
        deadline: 0,
        samples: false,
    };
}

/// What a level has to send at one poll, see [ObserveTable::plan].
#[derive(Debug, Clone)]
pub struct Plan<'a> {
    /// Earliest deadline first.
    due: &'a [Due],
    /// Whole bytes in the bucket.
    tokens: usize,
    /// The bucket is full.
    full: bool,
    missed: u32,
    t: Option<u64>,
}

impl<'a> Plan<'a> {
    pub fn is_empty(&self) -> bool {
        self.due.is_empty()
    }

    /// Slots of the due properties, earliest deadline first.
    pub fn edf(&self) -> impl Iterator<Item = usize> + Clone + 'a {
        self.due.iter().map(|d| d.k)
    }

    /// Whether slot `k` is due.
    pub fn is_due(&self, k: usize) -> bool {
        self.due.iter().any(|d| d.k == k)
    }

    /// Nothing is due, but sends or samples were missed: an update with no properties carries the count alone
    /// (`overhead` as for [selection](Self::selection)) when the budget has room for it. A count that waited for
    /// the next update would never reach the host when nothing else comes, as after the last samples of a burst
    /// the sampler lost.
    pub fn report_missed(&self, desc: &LevelDesc<'_>, overhead: usize) -> bool {
        self.due.is_empty() && self.missed > 0 && self.tokens >= self.missed_need(desc, overhead)
    }

    /// The bytes [report_missed](Self::report_missed) waits for in the bucket, for [ObserveTable::hold].
    pub fn missed_need(&self, desc: &LevelDesc<'_>, overhead: usize) -> usize {
        overhead + EVENT_SLACK + desc.header_bound()
    }

    /// `missed` of the update: sends missed before this poll, None when there were none.
    pub fn missed(&self) -> Option<UNib32> {
        (self.missed > 0).then_some(UNib32(self.missed))
    }

    /// `t` of the update, with [ObserveLimits::timestamps]: microseconds as a wrapping `u32`, as stream heads have
    /// it.
    pub fn t(&self) -> Option<ww_si::Second> {
        self.t.map(micros)
    }

    /// Start choosing what goes into the update: `overhead` is the length of the event without data (see
    /// [event_overhead]), `buf_len` the most the data may take. `marks` (one per slot of the level) is where the
    /// choice is kept.
    pub fn selection<'m>(
        &self,
        desc: &LevelDesc<'_>,
        overhead: usize,
        buf_len: usize,
        marks: &'m mut [Mark],
    ) -> Selection<'m> {
        let fixed = overhead + EVENT_SLACK + desc.header_bound();
        marks.fill(Mark::NONE);
        Selection {
            marks,
            count: 0,
            est: fixed,
            limit: self.tokens,
            max_len: buf_len.saturating_add(overhead),
            fixed,
            full: self.full,
            held: false,
            need: 0,
            done: false,
            time: false,
        }
    }
}

/// `t` of an update: microseconds as a wrapping `u32`, as stream heads have it.
fn micros(us: u64) -> ww_si::Second {
    ww_si::Second::from_parts(
        ww_si::Prefix::Micro,
        ww_si::ww_numeric::NumericValue::U32(us as u32),
    )
}

/// Whether a slot goes into an update, see [Selection]: storage for the selection, one per slot of the level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mark(u8);

impl Mark {
    /// Not offered, or left out: waits for the next update.
    pub const NONE: Mark = Mark(0);
    /// Goes into the update.
    const CHOSEN: u8 = 1;
    /// Can't be sent this time (value not readable, bigger than a message), moved on as missed.
    const DROPPED: u8 = 2;
    /// Goes into the update as a column of the level's batch of samples (API-39), with `CHOSEN`.
    const COLUMN: u8 = 4;

    fn is_chosen(self) -> bool {
        self.0 & Self::CHOSEN != 0
    }

    fn is_dropped(self) -> bool {
        self.0 & Self::DROPPED != 0
    }

    fn is_column(self) -> bool {
        self.0 & Self::COLUMN != 0
    }
}

/// The properties chosen for one update, offered in [Plan::edf] order with their estimated size.
#[derive(Debug)]
pub struct Selection<'m> {
    marks: &'m mut [Mark],
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
    /// Held: the bytes the update would have taken with the property that didn't fit.
    need: usize,
    done: bool,
    /// A batch of samples goes into the update: its time column is the event's `sample_times`.
    time: bool,
}

impl Selection<'_> {
    /// Offer slot `k` with the upper bound of its value's size, None when the value can't be had. Returns false
    /// once the update is full: offer nothing more.
    pub fn offer(&mut self, k: usize, size: Option<usize>) -> bool {
        self.offer_all(&[k], size, false)
    }

    /// Offer a batch of samples (API-39): the slots `ks` of its columns and its time column (the event's
    /// `sample_times`), all or none, `size` the upper bound of them all. Returns false once the update is full:
    /// offer nothing more.
    pub fn offer_batch(&mut self, ks: &[usize], size: usize) -> bool {
        self.offer_all(ks, Some(size), true)
    }

    fn offer_all(&mut self, ks: &[usize], size: Option<usize>, time: bool) -> bool {
        if ks.iter().any(|&k| k >= self.marks.len()) {
            self.done = true;
        }
        if self.done {
            return false;
        }
        let Some(size) = size else {
            self.mark(ks, Mark(Mark::DROPPED));
            return true;
        };
        let est = self.est.saturating_add(size);
        if est <= self.limit && est <= self.max_len {
            self.choose(ks, est, time);
            true
        } else if self.fixed.saturating_add(size) > self.max_len {
            // never fits a message
            self.mark(ks, Mark(Mark::DROPPED));
            true
        } else if self.count == 0 && self.full {
            // bigger than the bucket: alone, when the bucket is full
            self.choose(ks, est, time);
            self.done = true;
            false
        } else if !self.full && est <= self.max_len {
            // the bucket is what limits and it isn't full: hold, send nothing, rather than part of what is due
            for m in self.marks.iter_mut() {
                if m.is_chosen() {
                    *m = Mark::NONE;
                }
            }
            self.count = 0;
            self.time = false;
            self.held = true;
            self.need = est;
            self.done = true;
            false
        } else {
            self.done = true;
            false
        }
    }

    fn mark(&mut self, ks: &[usize], mark: Mark) {
        for &k in ks {
            self.marks[k] = mark;
        }
    }

    fn choose(&mut self, ks: &[usize], est: usize, time: bool) {
        let column = if time { Mark::COLUMN } else { 0 };
        self.mark(ks, Mark(Mark::CHOSEN | column));
        self.count += ks.len();
        self.est = est;
        self.time |= time;
    }

    /// Whether a batch of samples goes into the update, its time column as the event's `sample_times`.
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
        self.marks.get(k).is_some_and(|m| m.is_chosen())
    }

    /// Whether slot `k` can't be sent this time (its value can't be had, or it never fits a message): moved on as
    /// missed.
    pub fn is_dropped(&self, k: usize) -> bool {
        self.marks.get(k).is_some_and(|m| m.is_dropped())
    }

    /// Slot `k` could not be written after all: moved on as missed.
    pub fn drop_slot(&mut self, k: usize) {
        if let Some(m) = self.marks.get_mut(k) {
            *m = Mark(Mark::DROPPED);
        }
    }

    /// Resource indices of the chosen properties, increasing: the present slots of the update's `OptionAnyVec`.
    pub fn present<'a>(&'a self, desc: &LevelDesc<'a>) -> Present<'a> {
        Present {
            observable: desc.observable.iter(),
            marks: self.marks.iter(),
        }
    }
}

/// The writer of an update's values, as [write](fn@write) gives it to the level's value callback.
pub type UpdateWriter<'b, 'd> = OptionAnyVecWriter<'b, Present<'d>>;

/// Resource indices of the chosen properties of an update, increasing: see [Selection::present].
#[derive(Debug, Clone)]
pub struct Present<'a> {
    observable: core::slice::Iter<'a, u32>,
    marks: core::slice::Iter<'a, Mark>,
}

impl Iterator for Present<'_> {
    type Item = u32;

    fn next(&mut self) -> Option<u32> {
        loop {
            let (index, mark) = (self.observable.next()?, self.marks.next()?);
            if mark.is_chosen() {
                return Some(*index);
            }
        }
    }
}

/// The oldest batch of samples of a level (API-39), as [select] offers it: the table slots of its columns and the
/// bytes they all take in an update.
#[derive(Debug, Clone, Copy)]
pub struct BatchOffer<'k> {
    ks: &'k [usize],
    size: usize,
}

impl<'k> BatchOffer<'k> {
    /// The columns of `batch`, their slots put into `ks` (one per slot of the level).
    pub fn new(batch: &crate::batch::LevelBatch<'_>, ks: &'k mut [usize]) -> Self {
        let mut len = 0;
        let mut size = batch_slot_len(batch.time.len());
        for column in batch.columns() {
            if let Some(k) = ks.get_mut(len) {
                *k = column.k as usize;
                len += 1;
            }
            size += batch_slot_len(column.bytes.len());
        }
        let ks: &'k [usize] = ks;
        BatchOffer {
            ks: &ks[..len],
            size,
        }
    }
}

/// [ObserveTable::plan] of a level whose oldest batch of samples (API-39) is `oldest`: a slot in `Samples` mode is
/// due while the batch has a column of it.
pub fn plan_level<'a>(
    table: &mut ObserveTable,
    oldest: Option<&crate::batch::LevelBatch<'_>>,
    now_us: u64,
    limits: &ObserveLimits,
    due: &'a mut [Due],
) -> Plan<'a> {
    let Some(batch) = oldest else {
        return table.plan(now_us, limits, due);
    };
    // slots come in order: each lookup goes on from the last one
    let at = core::cell::Cell::new(0usize);
    table.plan_with(
        now_us,
        limits,
        &|k| {
            let mut a = at.get();
            let found = batch.column_after(k as u16, &mut a).is_some();
            at.set(a);
            found
        },
        due,
    )
}

/// One update of a level being put together (SRV-24), from [select] to [ObserveTable::commit].
#[derive(Debug)]
pub struct Round<'m> {
    sel: Selection<'m>,
    /// Nothing due: the update carries the missed count alone.
    missed_only: bool,
    /// The batch offered never fits a message: it goes, its samples missed.
    batch_dropped: bool,
}

impl<'m> Round<'m> {
    /// What goes into the update, for [ObserveTable::commit].
    pub fn selection(&self) -> &Selection<'m> {
        &self.sel
    }

    /// Whether slot `k` goes into the update with a value of its own (not a column of a batch of samples): what
    /// the level reads before [write](fn@write).
    pub fn needs_value(&self, k: usize) -> bool {
        self.sel
            .marks
            .get(k)
            .is_some_and(|m| m.is_chosen() && !m.is_column())
    }

    /// Whether the update carries a batch of samples (API-39): it leaves the outbox once sent.
    pub fn has_batch(&self) -> bool {
        self.sel.time
    }

    /// Whether the batch offered never fits a message: it leaves the outbox, its samples counted as missed.
    pub fn batch_dropped(&self) -> bool {
        self.batch_dropped
    }
}

/// An update of a level written by [write](fn@write), to send with [ser_update].
#[derive(Debug, Clone, Copy)]
pub struct Update {
    data_len: usize,
    t: Option<u64>,
    missed: Option<UNib32>,
    sample_times: bool,
}

impl Update {
    /// Length of the update's values at the start of the buffer [write](fn@write) wrote them to.
    pub fn data_len(&self) -> usize {
        self.data_len
    }

    /// Whether the event carries the time column of a batch of samples (`sample_times`, API-39).
    pub fn has_sample_times(&self) -> bool {
        self.sample_times
    }
}

/// What goes into the next update of a level (SRV-24), chosen from `plan` within the budget and `buf`: due
/// properties earliest deadline first, `size(k, scratch)` the upper bound of the bytes slot `k` takes (None: its
/// value can't be had), a slot in `Samples` mode offering `batch`, the level's oldest batch, with all its columns.
/// `marks` keeps the choice (one per slot). [Selected::Nothing] when the level has nothing to send at this poll,
/// [Selected::Hold] when only a missed count is due and waits for the bytes it takes (SRV-25: give them to
/// [ObserveTable::hold], so `next_wake` waits for the bucket).
pub fn select<'m>(
    plan: &Plan<'_>,
    desc: &LevelDesc<'_>,
    path: &[UNib32],
    buf: &mut [u8],
    marks: &'m mut [Mark],
    batch: Option<&BatchOffer<'_>>,
    size: &mut dyn FnMut(usize, &mut [u8]) -> Option<usize>,
) -> Result<Selected<'m>, ShrinkWrapError> {
    if plan.is_empty() && plan.missed == 0 {
        return Ok(Selected::Nothing);
    }
    let overhead = event_overhead(buf, path, plan.t(), plan.missed())?;
    // nothing due: the missed count goes alone, or waits for budget
    let missed_only = plan.report_missed(desc, overhead);
    if plan.is_empty() && !missed_only {
        return Ok(Selected::Hold(plan.missed_need(desc, overhead)));
    }
    let mut sel = plan.selection(desc, overhead, buf.len(), marks);
    let mut batch_offered = false;
    let mut batch_dropped = false;
    for due in plan.due {
        if due.samples {
            // the level's oldest batch, its time column and every column, once
            if batch_offered {
                continue;
            }
            batch_offered = true;
            let Some(batch) = batch else {
                continue;
            };
            let fits = sel.offer_batch(batch.ks, batch.size);
            batch_dropped = batch.ks.first().is_some_and(|&k| sel.is_dropped(k));
            if !fits {
                break;
            }
            continue;
        }
        if !sel.offer(due.k, size(due.k, buf)) {
            break;
        }
    }
    Ok(Selected::Round(Round {
        sel,
        missed_only,
        batch_dropped,
    }))
}

/// What [select] made of a level's plan.
#[derive(Debug)]
pub enum Selected<'m> {
    /// An update to write, send and commit.
    Round(Round<'m>),
    /// Nothing to send at this poll.
    Nothing,
    /// Only a missed count is due, and it waits for this many bytes in the bucket: [ObserveTable::hold] them.
    Hold(usize),
}

impl<'m> Selected<'m> {
    /// The update, if there is one.
    pub fn into_round(self) -> Option<Round<'m>> {
        match self {
            Selected::Round(round) => Some(round),
            Selected::Nothing | Selected::Hold(_) => None,
        }
    }
}

/// The values of the update `round` chose, into `buf`: `value(k, column, wr)` writes slot `k`'s value (`column`:
/// its column of the batch of samples), false when it can't. None when nothing goes out: nothing chosen, or a value
/// that couldn't be written (the chosen ones count as missed at the commit).
pub fn write(
    round: &Round<'_>,
    plan: &Plan<'_>,
    desc: &LevelDesc<'_>,
    buf: &mut [u8],
    value: &mut dyn FnMut(usize, bool, &mut UpdateWriter<'_, '_>) -> bool,
) -> Option<Update> {
    let sel = &round.sel;
    if sel.is_empty() && !round.missed_only {
        return None;
    }
    let mut wr = OptionAnyVecWriter::new(buf, sel.present(desc)).ok()?;
    for (k, mark) in sel.marks.iter().enumerate() {
        if mark.is_chosen() && !value(k, mark.is_column(), &mut wr) {
            return None;
        }
    }
    let data_len = wr.finish_and_take().ok()?.len();
    // the update's `t` is the time of its values: none when it carries only a batch, whose samples have their
    // times in `sample_times`
    let one_value = sel.marks.iter().any(|m| m.is_chosen() && !m.is_column());
    let has_batched = desc.batched.iter().any(|b| *b);
    Some(Update {
        data_len,
        t: if has_batched && !one_value {
            None
        } else {
            plan.t
        },
        missed: plan.missed(),
        sample_times: sel.time,
    })
}

/// The `PropertyUpdate` event of `update` for the level at `path` into `scratch`, its values `data` (what [write](fn@write)
/// wrote), with the time column of the batch sent (`sample_times`, API-39). Returns the event's length.
pub fn ser_update(
    scratch: &mut [u8],
    path: &[UNib32],
    update: &Update,
    data: &[u8],
    sample_times: Option<&[u8]>,
) -> Result<usize, ShrinkWrapError> {
    let t = update.t.map(micros);
    let sample_times = if update.sample_times {
        sample_times
    } else {
        None
    };
    ww_client_server::observe::ser_property_update_event(
        scratch,
        path,
        t,
        update.missed,
        sample_times,
        |b: &mut [u8]| {
            let b = b
                .get_mut(..data.len())
                .ok_or(ShrinkWrapError::OutOfBoundsWriteRawSlice)?;
            b.copy_from_slice(data);
            Ok(data.len())
        },
    )
    .map(|event| event.len())
}

/// Most bytes one level batch of samples (API-39) may take in an update of a level, its time column and `columns`
/// value columns together, so that it fits `buf_len` bytes of data next to an `overhead` ([event_overhead]): the
/// update's presence header, each slot's length and `Batched` tag and the `sample_times` flag and length left out. The batcher seals at it; an outbox of
/// `outbox_bytes` can hold no more than what is left of it after the entries' headers.
pub fn batch_budget(
    desc: &LevelDesc<'_>,
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
        // wrapping microseconds: the device clock and the grid wrap together
        let Some(us) = t.to_u32_wrapping_in(ww_si::Prefix::Micro) else {
            return true;
        };
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
    ww_client_server::observe::ser_property_update_event(buf, path, t, missed, None, |_| Ok(0))
        .map(|event| event.len())
}

#[cfg(test)]
mod tests;

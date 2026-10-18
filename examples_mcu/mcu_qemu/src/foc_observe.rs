//! Shared body of the observe cells benches (API-39 D8): the 21 `+observe+batched` properties of the motor demo's
//! FOC level ([foc_observe_api::Foc], values from [super::foc_signals]) sampled the default way,
//! `RootSampler::sample(t, FocSamples { .. })` into the two stores of `ObserveBatches`, and on observe cells,
//! `RootWriter::<prop>(..)` x 21 and `commit(t)` into a ring of atomics, by two blocking servers of the same API.
//! A host observes every property in `Samples` mode (every sample, sealed by count); `poll_properties` runs every
//! [POLL_EVERY] rows as an event loop would. Used by the `qemu-foc-observe` bins (instructions, counted from outside
//! with `qemu_icount`) and by the H725 bin `stm32h725ig-foc-observe-cycles` (DWT cycles), through [Meter].
//!
//! Per variant: [CLOSED_ROWS] rows with the gate closed (nobody observes), then [ROWS] rows with it open, each one
//! measured on its own (a row changes the state, so it can't be repeated), and every poll. Then the stack: the
//! same calls once more with the stack painted below the stack pointer ([stack_of]).

// each bin uses a part
#![allow(dead_code)]

use super::foc_signals as sig;
use core::hint::black_box;
use wire_weaver::prelude::*;
use ww_client_server::observe::{ObserveEntry, ObserveMode, ObserveRequest};
use ww_client_server::{PathKind, Request, RequestKind};
use ww_si::{Ampere, Radian, Ratio, Second, Volt, Weber};
use ww_stream::batch::{DEFAULT_POLICY, Scratch, scratch_bytes};
use ww_stream::observe::ObserveLimits;

/// Rows sampled with the gate open: two batches of `OBSERVE_BATCH` (32).
pub const ROWS: usize = 64;
/// Rows sampled before the host observes.
pub const CLOSED_ROWS: usize = 8;
/// Rows between two `poll_properties`: 640 µs of a 25 kHz loop.
pub const POLL_EVERY: usize = 16;
/// Rows of the cells ring: `RING - 1` can wait between two polls.
pub const RING: usize = 32;
/// Rows and polls run again for the stack.
pub const STACK_ROWS: usize = 4;
/// Bytes painted below the stack pointer.
pub const PAINT: usize = 8 * 1024;

/// Bytes of the message `poll_properties` builds.
const BUF: usize = 2048;
/// Where batches are rebuilt while they are encoded, one column at a time: the biggest column type.
const SB: usize = {
    let time = scratch_bytes::<Second>(32);
    let value = scratch_bytes::<Ampere>(32);
    if time > value { time } else { value }
};

const LIMITS: ObserveLimits = ObserveLimits {
    min_period_us: 100,
    default_budget: 10_000_000,
    max_level_budget: 10_000_000,
    total_budget: 10_000_000,
    timestamps: true,
    samples_max_age_us: (1_000, 1_000_000),
    samples_min_every_us: 0,
};

/// Which server.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    /// `RootSampler::sample` into `ObserveBatches`' two stores.
    Default,
    /// Observe cells: `RootWriter` setters and `commit` into a ring.
    Cells,
}

impl Variant {
    pub const ALL: [Variant; 2] = [Variant::Default, Variant::Cells];

    pub fn label(self) -> &'static str {
        match self {
            Variant::Default => "default",
            Variant::Cells => "cells",
        }
    }
}

/// What a measured call is.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Case {
    /// One row, nobody observes: the values and the gate check.
    Closed,
    /// One row, the host observes every sample.
    Row,
    /// One `poll_properties`: the cells' ring read into the staging store (cells), the sealed batch encoded and
    /// sent (both).
    Poll,
}

impl Case {
    pub const ALL: [Case; 3] = [Case::Closed, Case::Row, Case::Poll];

    pub fn label(self) -> &'static str {
        match self {
            Case::Closed => "closed",
            Case::Row => "row",
            Case::Poll => "poll",
        }
    }
}

/// Counts what a call costs.
pub trait Meter {
    /// Runs `work` once and counts it under `variant` and `case`; `k` counts the calls of a case from 0.
    fn measure(&mut self, variant: Variant, case: Case, k: usize, work: &mut dyn FnMut());
}

/// What a variant sent and needs.
pub struct Report {
    /// Bytes of every update sent.
    pub bytes: usize,
    /// FNV-1a of them: the same for both variants (same wire).
    pub hash: u32,
    /// Updates sent.
    pub updates: usize,
    /// Most stack a row took ([stack_of] of the row call minus that of an empty call), per case.
    pub stack: [usize; 3],
    /// Bytes of what the level's samples live in: `ObserveBatches` (and the `RootCells` ring).
    pub ram: usize,
}

/// Collects what `poll_properties` sends.
struct Wire<'a>(&'a mut Report);

impl wire_weaver::BlockingMessageSink for Wire<'_> {
    fn send(&mut self, message: &[u8]) -> Result<(), ()> {
        for b in message {
            self.0.hash = (self.0.hash ^ *b as u32).wrapping_mul(0x0100_0193);
        }
        self.0.bytes += message.len();
        self.0.updates += 1;
        Ok(())
    }
}

/// The getters of the level, never called: the host only observes in `Samples` mode.
macro_rules! getters {
    ($($get:ident: $ty:ty),* $(,)?) => {
        $(
            fn $get(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> GetResult<$ty, ()> {
                Unimplemented.into()
            }
        )*
    };
}

macro_rules! foc_getters {
    () => {
        getters!(
            get_i_a: Ampere, get_i_b: Ampere, get_i_c: Ampere, get_i_alpha: Ampere, get_i_beta: Ampere,
            get_i_d: Ampere, get_i_q: Ampere, get_v_d: Volt, get_v_q: Volt, get_v_alpha: Volt, get_v_beta: Volt,
            get_duty_a: Ratio, get_duty_b: Ratio, get_duty_c: Ratio, get_phase: Radian,
            get_phase_observer: Radian, get_phase_encoder: Radian, get_observer_x1: Weber,
            get_observer_x2: Weber, get_v_bus: Volt, get_phase_true: Radian,
        );
    };
}

/// The default server: the batches split, the sampler for the "interrupt", the consumers in `ObserveState`.
pub mod default {
    use super::*;

    pub type State = api_impl::ObserveState<api_impl::ObserveBatchConsumers<'static>>;

    pub struct Server {
        pub observe: State,
    }

    impl Server {
        foc_getters!();

        fn observe_state(&mut self) -> &mut State {
            &mut self.observe
        }
    }

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            foc_observe_api :: Foc for super::Server,
            server = true, no_alloc = true, use_async = false,
            method_model = "_=immediate",
            property_model = "_=get_set",
            observe_batch = 32,
            observe_outbox = 2048,
        );
    }
}

/// The cells server: the level on observe cells (`observe_cells = "root"`), its ring in a `static`.
pub mod cells {
    use super::*;

    pub static ROOT: api_impl::RootCells<RING> = api_impl::RootCells::new();

    pub type State = api_impl::ObserveState<api_impl::ObserveBatches<'static>>;

    pub struct Server {
        pub observe: State,
    }

    impl Server {
        foc_getters!();

        fn observe_state(&mut self) -> &mut State {
            &mut self.observe
        }
    }

    pub mod api_impl {
        wire_weaver::ww_codegen!(
            foc_observe_api :: Foc for super::Server,
            server = true, no_alloc = true, use_async = false,
            method_model = "_=immediate",
            property_model = "_=get_set",
            observe_batch = 32,
            observe_outbox = 2048,
            observe_cells = "root",
        );
    }
}

/// Device time of row `i`.
fn t(i: usize) -> Second {
    Second::micro(sig::T0_US.wrapping_add(i as u32 * sig::DT_US))
}

fn now_us(i: usize) -> u64 {
    (sig::T0_US + i as u32 * sig::DT_US) as u64
}

/// One row the default way: what a FOC interrupt does at its end.
#[inline(never)]
fn sample_default(s: &mut default::api_impl::RootSampler<'_>, i: usize) -> usize {
    let i = black_box(i);
    let k = i % sig::I_A.len();
    s.sample(
        t(i),
        default::api_impl::FocSamples {
            i_a: Ampere::milli(sig::I_A[k]),
            i_b: Ampere::milli(sig::I_B[k]),
            i_c: Ampere::milli(sig::I_C[k]),
            i_alpha: Ampere::milli(sig::I_ALPHA[k]),
            i_beta: Ampere::milli(sig::I_BETA[k]),
            i_d: Ampere::milli(sig::I_D[k]),
            i_q: Ampere::milli(sig::I_Q[k]),
            v_d: Volt::milli(sig::V_D[k]),
            v_q: Volt::milli(sig::V_Q[k]),
            v_alpha: Volt::milli(sig::V_ALPHA[k]),
            v_beta: Volt::milli(sig::V_BETA[k]),
            duty_a: Ratio::micro(sig::DUTY_A[k]),
            duty_b: Ratio::micro(sig::DUTY_B[k]),
            duty_c: Ratio::micro(sig::DUTY_C[k]),
            phase: Radian::micro(sig::PHASE[k]),
            phase_observer: Radian::micro(sig::PHASE_OBSERVER[k]),
            phase_encoder: Radian::micro(sig::PHASE_ENCODER[k]),
            observer_x1: Weber::nano(sig::OBSERVER_X1[k]),
            observer_x2: Weber::nano(sig::OBSERVER_X2[k]),
            v_bus: Volt::milli(sig::V_BUS[k]),
            phase_true: Radian::micro(sig::PHASE_TRUE[k]),
        },
    )
}

/// One row on observe cells: each value put where it is computed, then the row committed.
#[inline(never)]
fn sample_cells(w: &mut cells::api_impl::RootWriter<'_, RING>, i: usize) -> bool {
    let i = black_box(i);
    let k = i % sig::I_A.len();
    w.i_a(Ampere::milli(sig::I_A[k]));
    w.i_b(Ampere::milli(sig::I_B[k]));
    w.i_c(Ampere::milli(sig::I_C[k]));
    w.i_alpha(Ampere::milli(sig::I_ALPHA[k]));
    w.i_beta(Ampere::milli(sig::I_BETA[k]));
    w.i_d(Ampere::milli(sig::I_D[k]));
    w.i_q(Ampere::milli(sig::I_Q[k]));
    w.v_d(Volt::milli(sig::V_D[k]));
    w.v_q(Volt::milli(sig::V_Q[k]));
    w.v_alpha(Volt::milli(sig::V_ALPHA[k]));
    w.v_beta(Volt::milli(sig::V_BETA[k]));
    w.duty_a(Ratio::micro(sig::DUTY_A[k]));
    w.duty_b(Ratio::micro(sig::DUTY_B[k]));
    w.duty_c(Ratio::micro(sig::DUTY_C[k]));
    w.phase(Radian::micro(sig::PHASE[k]));
    w.phase_observer(Radian::micro(sig::PHASE_OBSERVER[k]));
    w.phase_encoder(Radian::micro(sig::PHASE_ENCODER[k]));
    w.observer_x1(Weber::nano(sig::OBSERVER_X1[k]));
    w.observer_x2(Weber::nano(sig::OBSERVER_X2[k]));
    w.v_bus(Volt::milli(sig::V_BUS[k]));
    w.phase_true(Radian::micro(sig::PHASE_TRUE[k]));
    w.commit(t(i))
}

/// The `Observe` request of a host asking for every sample of every property of the level.
fn observe_request(buf: &mut [u8]) -> &[u8] {
    let entries: [ObserveEntry; 21] = core::array::from_fn(|k| ObserveEntry {
        index: UNib32(k as u32),
        mode: ObserveMode::Samples {
            every_us: UNib32(0),
            max_age_us: UNib32(1_000_000),
        },
    });
    let req = Request {
        seq: UVlq32Backfill(1),
        path_kind: PathKind::Absolute {
            path: RefVec::Slice { slice: &[] },
        },
        kind: RequestKind::Observe {
            cfg: ObserveRequest {
                entries: RefVec::Slice { slice: &entries },
                patch: false,
                budget: None,
            },
        },
    };
    req.to_ww_bytes(buf).unwrap()
}

/// Replies to requests, dropped.
struct Drop;

impl wire_weaver::BlockingMessageSink for Drop {
    fn send(&mut self, _message: &[u8]) -> Result<(), ()> {
        Ok(())
    }
}

/// Stack bytes `work` takes: the words below the stack pointer are painted, `work` runs, the lowest word changed
/// is how deep it went. Includes this call's own frame for `work`: subtract [stack_of] of an empty `work`.
#[inline(never)]
pub fn stack_of(work: &mut dyn FnMut()) -> usize {
    const PATTERN: u32 = 0xA5C3_5A3C;
    // the words below what this frame and its calls may already use
    let top = (cortex_m::register::msp::read() as usize - 64) & !3;
    let bottom = top - PAINT;
    for a in (bottom..top).step_by(4) {
        // SAFETY: free stack below the stack pointer, nothing lives there
        unsafe { core::ptr::write_volatile(a as *mut u32, PATTERN) };
    }
    work();
    let mut low = top;
    for a in (bottom..top).step_by(4) {
        // SAFETY: as above
        if unsafe { core::ptr::read_volatile(a as *const u32) } != PATTERN {
            low = a;
            break;
        }
    }
    top - low
}

/// Runs both variants, measures every call with `meter`, returns what each sent and needs.
pub fn run(meter: &mut dyn Meter) -> [Report; 2] {
    let empty = stack_of(&mut || {});
    let mut req = [0u8; 256];
    let req = observe_request(&mut req);
    let mut scratch = [0u8; 256];
    let mut event_scratch = [0u8; BUF + 64];
    let mut buf = [0u8; BUF];
    let mut sb = Scratch::<SB>::new();
    let mut reports = [Variant::Default, Variant::Cells].map(|_| Report {
        bytes: 0,
        hash: 0x811c_9dc5,
        updates: 0,
        stack: [0; 3],
        ram: 0,
    });

    // default: the batches split, the sampler is the interrupt's
    {
        let v = Variant::Default;
        let report = &mut reports[0];
        let batches = cortex_m::singleton!(
            : default::api_impl::ObserveBatches = default::api_impl::ObserveBatches::new(DEFAULT_POLICY)
        )
        .unwrap();
        report.ram = core::mem::size_of::<default::api_impl::ObserveBatches>();
        let (samplers, consumers) = batches.split();
        let mut sampler = samplers.root;
        let mut server = default::Server {
            observe: default::State::new(LIMITS, consumers),
        };
        for i in 0..CLOSED_ROWS {
            meter.measure(v, Case::Closed, i, &mut || {
                black_box(sample_default(&mut sampler, i));
            });
        }
        let mut out = EventWriter::new(Drop, &mut event_scratch);
        server
            .process_request_bytes(req, &mut scratch, &mut out, ())
            .unwrap();
        let mut polls = 0;
        for i in 0..ROWS {
            meter.measure(v, Case::Row, i, &mut || {
                black_box(sample_default(&mut sampler, i));
            });
            if (i + 1) % POLL_EVERY == 0 {
                let mut out = EventWriter::new(Wire(report), &mut event_scratch);
                meter.measure(v, Case::Poll, polls, &mut || {
                    server
                        .poll_properties(&mut out, (), now_us(i), &mut buf, &mut sb)
                        .unwrap();
                });
                polls += 1;
            }
        }
        // the stack: rows on, polls with nothing to send
        for i in ROWS..ROWS + STACK_ROWS {
            let s = stack_of(&mut || {
                black_box(sample_default(&mut sampler, i));
            });
            report.stack[Case::Row as usize] = report.stack[Case::Row as usize].max(s - empty);
        }
        let mut sink = Report {
            bytes: 0,
            hash: 0,
            updates: 0,
            stack: [0; 3],
            ram: 0,
        };
        let mut out = EventWriter::new(Wire(&mut sink), &mut event_scratch);
        let s = stack_of(&mut || {
            server
                .poll_properties(&mut out, (), now_us(ROWS + STACK_ROWS), &mut buf, &mut sb)
                .unwrap();
        });
        report.stack[Case::Poll as usize] = s - empty;
    }

    // cells: the writer is the interrupt's, the reader in ObserveBatches
    {
        let v = Variant::Cells;
        let report = &mut reports[1];
        report.ram = core::mem::size_of::<cells::api_impl::ObserveBatches<'static>>()
            + core::mem::size_of::<cells::api_impl::RootCells<RING>>();
        let mut writer = cells::ROOT.writer().unwrap();
        let mut server = cells::Server {
            observe: cells::State::new(
                LIMITS,
                cells::api_impl::ObserveBatches::new(DEFAULT_POLICY, cells::ROOT.reader().unwrap()),
            ),
        };
        for i in 0..CLOSED_ROWS {
            meter.measure(v, Case::Closed, i, &mut || {
                black_box(sample_cells(&mut writer, i));
            });
        }
        let mut out = EventWriter::new(Drop, &mut event_scratch);
        server
            .process_request_bytes(req, &mut scratch, &mut out, ())
            .unwrap();
        let mut polls = 0;
        for i in 0..ROWS {
            meter.measure(v, Case::Row, i, &mut || {
                black_box(sample_cells(&mut writer, i));
            });
            if (i + 1) % POLL_EVERY == 0 {
                let mut out = EventWriter::new(Wire(report), &mut event_scratch);
                meter.measure(v, Case::Poll, polls, &mut || {
                    server
                        .poll_properties(&mut out, (), now_us(i), &mut buf, &mut sb)
                        .unwrap();
                });
                polls += 1;
            }
        }
        for i in ROWS..ROWS + STACK_ROWS {
            let s = stack_of(&mut || {
                black_box(sample_cells(&mut writer, i));
            });
            report.stack[Case::Row as usize] = report.stack[Case::Row as usize].max(s - empty);
        }
        let mut sink = Report {
            bytes: 0,
            hash: 0,
            updates: 0,
            stack: [0; 3],
            ram: 0,
        };
        let mut out = EventWriter::new(Wire(&mut sink), &mut event_scratch);
        let s = stack_of(&mut || {
            server
                .poll_properties(&mut out, (), now_us(ROWS + STACK_ROWS), &mut buf, &mut sb)
                .unwrap();
        });
        report.stack[Case::Poll as usize] = s - empty;
    }
    reports
}

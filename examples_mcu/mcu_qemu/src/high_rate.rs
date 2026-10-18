//! Shared body of the high-rate observe benches: a few values of a power converter observed at its switching rate,
//! three ([power_stage_api::PowerStage]) at 400 kHz and 1 MHz, two ([power_stage_api::PowerStage2]) at 2 MHz, values
//! from [super::power_signals]. One observe level, its shared clock, observe cells as the capture (API-50:
//! `RootWriter::<prop>(..)` per value and `commit(t)`, a device time in ns), the column-wise `DeltaPacked` writer as
//! the encode (SW-64, the `no_std` default), `observe_batch = 32`. A host observes every property in `Samples` mode
//! (every sample, sealed by count); `poll_properties` runs every [POLL_EVERY] rows, so every second poll only
//! drains the ring and the other also encodes and sends a batch.
//!
//! The reference is a raw copy: the "interrupt" stores the time and the values of a row into a plain ring of
//! words, and the "event loop" copies a batch of rows out of it with `copy_from_slice`, 4 bytes per value and
//! per time, no meaning, no framing.
//!
//! Used by `qemu-m4f-high-rate` (instructions, `qemu_icount`) and `stm32h725ig-high-rate-cycles` (DWT cycles),
//! through [Meter]. Each row and poll is measured on its own (they change the state).

// each bin uses a part
#![allow(dead_code)]

use super::power_signals as sig;
use core::hint::black_box;
use static_cell::StaticCell;
use wire_weaver::prelude::*;
use ww_client_server::observe::{ObserveEntry, ObserveMode, ObserveRequest};
use ww_client_server::{PathKind, Request, RequestKind};
use ww_si::{Ampere, Second, Volt};
use ww_stream::batch::{DEFAULT_POLICY, Scratch, scratch_bytes};
use ww_stream::observe::ObserveLimits;

/// Rows per run: two batches of `OBSERVE_BATCH` (32), the first quiet, the second a load step.
pub const ROWS: usize = sig::ROWS;
/// Rows between two `poll_properties`.
pub const POLL_EVERY: usize = 16;
/// Rows of the cells ring: `RING - 1` can wait between two polls.
pub const RING: usize = 32;
/// Rows per batch, as `observe_batch`.
pub const BATCH: usize = 32;
/// Device time of the first row, ns.
pub const T0_NS: u32 = 1_000_000_000;

/// Bytes of the message `poll_properties` builds, as `observe_outbox`: room for the worst case of a full batch, so
/// that the first batch (sealed by that bound, before any batch was measured) has its 32 rows too.
const BUF: usize = 4096;
/// Where batches are rebuilt while they are encoded, one column at a time.
const SB: usize = {
    let time = scratch_bytes::<Second>(32);
    let value = scratch_bytes::<Ampere>(32);
    if time > value { time } else { value }
};

const LIMITS: ObserveLimits = ObserveLimits {
    min_period_us: 100,
    default_budget: 100_000_000,
    max_level_budget: 100_000_000,
    total_budget: 100_000_000,
    timestamps: true,
    samples_max_age_us: (1_000, 1_000_000),
    samples_min_every_us: 0,
};

/// What is observed, at which rate.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Run {
    /// `i_l`, `v_out`, `v_in` at 400 kHz.
    Three400k,
    /// The same at 1 MHz.
    Three1M,
    /// `i_l`, `v_out` at 2 MHz.
    Two2M,
}

impl Run {
    pub const ALL: [Run; 3] = [Run::Three400k, Run::Three1M, Run::Two2M];

    pub fn label(self) -> &'static str {
        match self {
            Run::Three400k => "3 values at 400 kHz",
            Run::Three1M => "3 values at 1 MHz",
            Run::Two2M => "2 values at 2 MHz",
        }
    }

    /// Rows per second.
    pub fn rate_hz(self) -> u32 {
        match self {
            Run::Three400k => 400_000,
            Run::Three1M => 1_000_000,
            Run::Two2M => 2_000_000,
        }
    }

    /// The period, ns.
    pub fn dt_ns(self) -> u32 {
        1_000_000_000 / self.rate_hz()
    }

    /// Values per row.
    pub fn values(self) -> usize {
        match self {
            Run::Three400k | Run::Three1M => 3,
            Run::Two2M => 2,
        }
    }

    /// Bytes of a raw row: the time and the values, 4 bytes each.
    pub fn raw_row_bytes(self) -> usize {
        4 * (1 + self.values())
    }
}

/// What a measured call is.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Case {
    /// One row into the cells: the setters and `commit`.
    CellsRow,
    /// One `poll_properties` that drains the ring only.
    Drain,
    /// One `poll_properties` that drains the ring and encodes and sends a batch.
    Encode,
    /// One row stored raw into a ring of words.
    RawRow,
    /// One batch of raw rows copied out of the ring.
    RawBatch,
}

impl Case {
    pub const ALL: [Case; 5] = [
        Case::CellsRow,
        Case::Drain,
        Case::Encode,
        Case::RawRow,
        Case::RawBatch,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Case::CellsRow => "cells row",
            Case::Drain => "poll, drain only",
            Case::Encode => "poll, drain+encode",
            Case::RawRow => "raw row",
            Case::RawBatch => "raw batch copy",
        }
    }
}

/// Counts what a call costs.
pub trait Meter {
    /// Runs `work` once and counts it under `run` and `case`; `k` counts the calls of a case from 0.
    fn measure(&mut self, run: Run, case: Case, k: usize, work: &mut dyn FnMut());
}

/// What a run sent.
#[derive(Clone, Copy)]
pub struct Report {
    /// Bytes of every update sent.
    pub bytes: usize,
    /// FNV-1a of them.
    pub hash: u32,
    /// Updates sent.
    pub updates: usize,
}

impl Report {
    const fn new() -> Self {
        Report {
            bytes: 0,
            hash: 0x811c_9dc5,
            updates: 0,
        }
    }
}

/// Bytes of what the level's samples live in, per stage: `ObserveBatches` and the `RootCells` ring.
pub fn ram() -> [usize; 2] {
    [
        core::mem::size_of::<three::api_impl::ObserveBatches<'static>>()
            + core::mem::size_of::<three::api_impl::RootCells<RING>>(),
        core::mem::size_of::<two::api_impl::ObserveBatches<'static>>()
            + core::mem::size_of::<two::api_impl::RootCells<RING>>(),
    ]
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

/// Replies to requests, dropped.
struct Drop;

impl wire_weaver::BlockingMessageSink for Drop {
    fn send(&mut self, _message: &[u8]) -> Result<(), ()> {
        Ok(())
    }
}

/// One server per stage, its level on observe cells (`observe_cells = "root"`), its ring in a `static`.
macro_rules! stage {
    ($m:ident, $api:ident, $($get:ident: $ty:ty),*) => {
        pub mod $m {
            use super::*;

            pub static ROOT: api_impl::RootCells<RING> = api_impl::RootCells::new();

            pub static BATCHES: StaticCell<api_impl::ObserveBatches<'static>> = StaticCell::new();

            pub type State = api_impl::ObserveState<&'static mut api_impl::ObserveBatches<'static>>;

            pub static STATE: StaticCell<State> = StaticCell::new();

            pub struct Server {
                pub observe: &'static mut State,
            }

            impl Server {
                $(
                    fn $get(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> GetResult<$ty, ()> {
                        Unimplemented.into()
                    }
                )*

                fn observe_state(&mut self) -> &mut State {
                    self.observe
                }
            }

            /// The batches (the cells' reader in them) and the state built in place in their statics: the writer
            /// for the interrupt, the server.
            #[inline(never)]
            pub fn setup() -> (api_impl::RootWriter<'static, RING>, Server) {
                let batches = api_impl::ObserveBatches::init(
                    BATCHES.uninit(),
                    DEFAULT_POLICY,
                    ROOT.reader().unwrap(),
                );
                let observe = State::init(STATE.uninit(), LIMITS, batches);
                (ROOT.writer().unwrap(), Server { observe })
            }

            pub mod api_impl {
                wire_weaver::ww_codegen!(
                    power_stage_api :: $api for super::Server,
                    server = true, no_alloc = true, use_async = false,
                    method_model = "_=immediate",
                    property_model = "_=get_set",
                    observe_batch = 32,
                    observe_outbox = 4096,
                    observe_cells = "root",
                );
            }
        }
    };
}

stage!(three, PowerStage, get_i_l: Ampere, get_v_out: Volt, get_v_in: Volt);
stage!(two, PowerStage2, get_i_l: Ampere, get_v_out: Volt);

/// Device time of row `i` of `run`, ns; the runs follow each other on one clock.
fn t_ns(run: Run, i: usize) -> u32 {
    let start = match run {
        Run::Three400k | Run::Two2M => T0_NS,
        // after the 400 kHz rows on the same server
        Run::Three1M => T0_NS + ROWS as u32 * Run::Three400k.dt_ns(),
    };
    start.wrapping_add(i as u32 * run.dt_ns())
}

fn now_us(run: Run, i: usize) -> u64 {
    (t_ns(run, i) / 1000) as u64
}

/// One row of three on observe cells: what a 400 kHz or 1 MHz interrupt does once its values are computed.
#[inline(never)]
fn row_three(w: &mut three::api_impl::RootWriter<'_, RING>, run: Run, i: usize) -> bool {
    let i = black_box(i);
    let k = i % ROWS;
    w.i_l(Ampere::milli(sig::I_L[k]));
    w.v_out(Volt::milli(sig::V_OUT[k]));
    w.v_in(Volt::milli(sig::V_IN[k]));
    w.commit(Second::nano(t_ns(run, i)))
}

/// One row of two on observe cells.
#[inline(never)]
fn row_two(w: &mut two::api_impl::RootWriter<'_, RING>, run: Run, i: usize) -> bool {
    let i = black_box(i);
    let k = i % ROWS;
    w.i_l(Ampere::milli(sig::I_L[k]));
    w.v_out(Volt::milli(sig::V_OUT[k]));
    w.commit(Second::nano(t_ns(run, i)))
}

/// The raw reference: rows of words, the time first, then the values.
pub struct RawRing {
    words: [u32; RING * 4],
    head: usize,
}

/// One row stored raw: the time and `values` of the row's values.
#[inline(never)]
fn raw_row(ring: &mut RawRing, run: Run, i: usize) {
    let i = black_box(i);
    let k = i % ROWS;
    let w = 1 + run.values();
    let at = (ring.head % RING) * w;
    let row = &mut ring.words[at..at + w];
    row[0] = t_ns(run, i);
    row[1] = sig::I_L[k] as u32;
    row[2] = sig::V_OUT[k] as u32;
    if w == 4 {
        row[3] = sig::V_IN[k] as u32;
    }
    ring.head = ring.head.wrapping_add(1);
}

/// One batch of raw rows copied out of the ring as little-endian bytes: a plain copy on a little-endian target.
#[inline(never)]
fn raw_batch(ring: &RawRing, run: Run, buf: &mut [u8]) -> usize {
    let n = BATCH * (1 + run.values());
    #[cfg(target_endian = "little")]
    {
        // SAFETY: `n` initialized words, `u8` has no alignment
        let bytes = unsafe { core::slice::from_raw_parts(ring.words.as_ptr().cast::<u8>(), 4 * n) };
        buf[..4 * n].copy_from_slice(bytes);
    }
    #[cfg(not(target_endian = "little"))]
    for (d, v) in buf.chunks_exact_mut(4).zip(&ring.words[..n]) {
        d.copy_from_slice(&v.to_le_bytes());
    }
    4 * n
}

/// The `Observe` request of a host asking for every sample of the level's `n` properties.
fn observe_request(buf: &mut [u8], n: usize) -> &[u8] {
    let entries: [ObserveEntry; 3] = core::array::from_fn(|k| ObserveEntry {
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
                entries: RefVec::Slice {
                    slice: &entries[..n],
                },
                patch: false,
                budget: None,
            },
        },
    };
    req.to_ww_bytes(buf).unwrap()
}

/// Rows and polls of one run on a stage's server: `row` is the cells row, `poll` one `poll_properties`.
fn run_rows(
    meter: &mut dyn Meter,
    run: Run,
    report: &mut Report,
    row: &mut dyn FnMut(usize) -> bool,
    poll: &mut dyn FnMut(&mut Report, u64),
) {
    let mut polls = [0; 2];
    for i in 0..ROWS {
        meter.measure(run, Case::CellsRow, i, &mut || {
            black_box(row(i));
        });
        if (i + 1) % POLL_EVERY == 0 {
            let case = if (i + 1) % BATCH == 0 {
                Case::Encode
            } else {
                Case::Drain
            };
            let k = &mut polls[(case == Case::Encode) as usize];
            meter.measure(run, case, *k, &mut || poll(report, now_us(run, i)));
            *k += 1;
        }
    }
}

/// Runs every run, measures every call with `meter`, returns what each sent and how many raw bytes a batch is.
pub fn run(meter: &mut dyn Meter) -> [Report; 3] {
    let mut req = [0u8; 64];
    let mut scratch = [0u8; 256];
    let mut event_scratch = [0u8; BUF + 64];
    let mut buf = [0u8; BUF];
    let mut sb = Scratch::<SB>::new();
    let mut reports = [Report::new(); 3];

    // three values: 400 kHz, then 1 MHz on the same server and clock
    {
        let (mut w, mut server) = three::setup();
        let mut out = EventWriter::new(Drop, &mut event_scratch);
        server
            .process_request_bytes(observe_request(&mut req, 3), &mut scratch, &mut out, ())
            .unwrap();
        for (r, run) in [Run::Three400k, Run::Three1M].into_iter().enumerate() {
            run_rows(
                meter,
                run,
                &mut reports[r],
                &mut |i| row_three(&mut w, run, i),
                &mut |report, now| {
                    let mut out = EventWriter::new(Wire(report), &mut event_scratch);
                    server
                        .poll_properties(&mut out, (), now, &mut buf, &mut sb)
                        .unwrap();
                },
            );
        }
    }

    // two values at 2 MHz
    {
        let run = Run::Two2M;
        let (mut w, mut server) = two::setup();
        let mut out = EventWriter::new(Drop, &mut event_scratch);
        server
            .process_request_bytes(observe_request(&mut req, 2), &mut scratch, &mut out, ())
            .unwrap();
        run_rows(
            meter,
            run,
            &mut reports[2],
            &mut |i| row_two(&mut w, run, i),
            &mut |report, now| {
                let mut out = EventWriter::new(Wire(report), &mut event_scratch);
                server
                    .poll_properties(&mut out, (), now, &mut buf, &mut sb)
                    .unwrap();
            },
        );
    }

    // the raw reference
    let mut ring = RawRing {
        words: [0; RING * 4],
        head: 0,
    };
    for run in Run::ALL {
        ring.head = 0;
        for i in 0..ROWS {
            meter.measure(run, Case::RawRow, i, &mut || raw_row(&mut ring, run, i));
            if (i + 1) % BATCH == 0 {
                let ring = black_box(&ring);
                meter.measure(run, Case::RawBatch, i / BATCH, &mut || {
                    black_box(raw_batch(ring, run, black_box(&mut buf)));
                });
            }
        }
    }
    reports
}

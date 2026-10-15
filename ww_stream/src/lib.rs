#![cfg_attr(not(any(feature = "std", test)), no_std)]
//! Counted and reliable WireWeaver streams (API-30): positions in units, exact loss accounting, acknowledgements
//! and go-back-N resends, stores that keep data while the host is slow or gone, and backfill after a reconnect with
//! the latest values first. Design and reasons: `docs/api/stream_reliability.md`.
//!
//! Everything here is sans-IO: generated server and client code (or a test) feeds the state machines what arrived
//! and sends what they return, and passes the time in milliseconds where timing matters. The wire types live in
//! `ww_client_server::stream`.
//!
//! | Module | What | Used by |
//! |---|---|---|
//! | [tx] | [Counted](tx::Counted), [Reliable](tx::Reliable): sender state machines (runs, credit, go-back-N, sessions) | device |
//! | [rx] | [Receiver](rx::Receiver): in order per run, gaps by cause, duplicates, `Ack` timing | host |
//! | [store] | [StreamStore](store::StreamStore), [RamRing](store::RamRing) | device |
//! | [observe] | [ObserveTable](observe::ObserveTable): observed properties, budget, earliest deadline first (SRV-18) | device |
//! | [sched] | [Scheduler](sched::Scheduler): which stream sends next, by priority | device |
//! | [counters] | [StreamCnt](counters::StreamCnt) (`cnt` instance counters with the `cnt` feature), [TxStats](counters::TxStats) | device |
//!
//! **Sharing with producer tasks.** A [Reliable](tx::Reliable) is one `&mut` state: the producer pushes into it and
//! the event loop reads the next event out of it into its own buffer ([Reliable::next_event](tx::Reliable::next_event)),
//! then sends that buffer with the state released. Shared between tasks it sits behind the firmware's async mutex,
//! held only for a push or a store read, never while the link is busy: a slow link never stalls a producer.

pub mod counters;
pub mod observe;
pub mod rx;
pub mod sched;
pub mod store;
pub mod tx;

pub use ww_client_server::stream::{
    HeadExt, StoreStatus, StreamCursor, StreamHead, StreamOpened, StreamStatus,
};
/// Device time in the head (`t`), re-exported for generated code.
pub use ww_si::Second;

/// The two runs of positions of a resumed reliable stream, each delivered in order: the live run (the newest event
/// the device had at the resume and everything after it) and the backfill run (the backlog up to where the live run
/// starts). Streams that were not resumed only have a live run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Run {
    Live,
    Backfill,
}

/// How a stream was opened, the device side of [StreamOpened] (which adds the stream function's result).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Opened {
    /// Session the stream is in now.
    pub session: u32,
    /// Oldest position the device still has, `live_from` when it keeps nothing.
    pub oldest: u64,
    /// Where the live run starts: the newest event kept (or the units lost right before it), or the next one.
    pub live_from: u64,
    /// Where the backfill run starts, `None` without one; it ends at `live_from`.
    pub resume_from: Option<u64>,
}

impl Opened {
    /// The wire form, with the stream function's serialized result.
    pub fn to_wire<'i>(&self, result: &'i [u8]) -> StreamOpened<'i> {
        use wire_weaver::shrink_wrap::{UVlq32, UVlq64, tail_bytes::TailBytes};
        StreamOpened {
            session: UVlq32(self.session),
            oldest: UVlq64(self.oldest),
            live_from: UVlq64(self.live_from),
            resume_from: self.resume_from.map(UVlq64),
            result: TailBytes(result),
        }
    }

    /// From the wire form.
    pub fn from_wire(opened: &StreamOpened<'_>) -> Self {
        Opened {
            session: opened.session.0,
            oldest: opened.oldest.0,
            live_from: opened.live_from.0,
            resume_from: opened.resume_from.map(|p| p.0),
        }
    }
}

/// Why a stream couldn't be resumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResumeError<E> {
    /// The stream has no session to resume: the host has to open it with arguments.
    NoSession,
    /// The store failed.
    Store(E),
}

#[cfg(test)]
mod tests;

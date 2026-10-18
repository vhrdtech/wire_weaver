//! Batched streams and observed properties (API-41): items collected on the device as a struct of arrays and sent
//! as one `Series` per event.
//!
//! A [Batcher] holds two stores of `N` items each ([SeriesStore], no padding between fields), shared by a
//! [Producer] and a [Consumer] ([Batcher::split]). The producer [pushes](Producer::push) items into the filling
//! store: a copy of the item's fields and two atomic operations, never an encode and never a wait, so it may run in a
//! control loop or an interrupt. A batch is sealed when it has `N` items, when one more item could make it bigger
//! than the event budget, or (by the consumer) when it gets as old as the host allows; sealing hands the store over
//! to the consumer and the producer goes on in the spare one. If the spare isn't back yet, the batch's items are
//! lost (counted, [Seal::Lost]) and the store starts over.
//!
//! The consumer, the event loop in task context, [polls](Consumer::poll) (age, budget) and
//! [encodes](Consumer::encode_into) each sealed batch exactly once while the producer goes on: into a reliable
//! stream's store ([Consumer::poll_reliable]), into an event of a counted stream ([Consumer::poll_counted]), or into
//! a level's [Outbox] for observe; then it gives the store back. Encoding rebuilds the items into one [Scratch]
//! shared by every batcher of a server (one batch is encoded at a time) and writes them with `Series::with_policy`.
//! A batcher used from one task only needs no split: [Batcher] has the methods of both halves.
//!
//! **Hand-off.** One atomic word holds which store is filling, its item count, the sealed store's item count and
//! why it was sealed, and a busy flag the producer sets while it writes an item. A push is a `fetch_or` and a
//! compare-exchange around the copy; the consumer takes a sealed store with a load and gives it back with a
//! `fetch_and`, and seals the filling one (age, a smaller budget) with a compare-exchange while the producer isn't
//! busy. No lock is held while a batch is encoded. Targets without compare-exchange (`thumbv6m`) get it from
//! `portable-atomic` with its `critical-section` (or `unsafe-assume-single-core`) feature, a critical section of a
//! few instructions.
//!
//! **Budget.** The producer seals when the batch could pass the budget, at a limit the consumer works out and
//! publishes: the worst case of [Series::max_bytes](wire_weaver::shrink_wrap::series::Series::max_bytes) (always enough), or, once a batch was encoded, its bytes per item
//! with a margin, whichever allows more. That is an estimate, not a bound (a signal can turn noisier): a sealed
//! batch that turns out bigger than the room it is encoded into is split, the largest prefix that fits goes out and
//! the rest stays for the next call. So every encoded batch fits, and no item is lost or sent twice.
//!
//! **Age** counts from the first [poll](Consumer::poll) that saw the batch's first item (no clock in the push), so
//! a batch goes out at most `max_age` plus one poll interval after its first item.
//!
//! **Cost** on a Cortex-M3 (`examples_mcu/mcu_qemu`, bin `qemu-batch-cpu`, instructions counted in a QEMU trace),
//! `Timed<Ampere>` with [DEFAULT_POLICY]: a `push` about 62 instructions, the push that seals a full batch and hands
//! it over about 71, a `poll` that seals nothing about 108, one that seals a due batch about 159; `encode_into` of 32
//! items about 65 250, of which about 2 270 take the store, rebuild the items into the scratch and give the store
//! back, the rest is the `Series` encode itself. No interrupts are masked on the way (`LDREX`/`STREX`).

mod batcher;
mod level;
mod outbox;
mod stream;

pub use batcher::{BatchSource, Batcher, Bound, Consumer, MAX_BATCH, Polled, Producer, Push};
pub use level::{Columns, LevelRow, MAX_COLUMNS, RowsEncoded, column_bits};
pub use outbox::{LevelBatch, OUTBOX_ENTRY_HEADER, Outbox, OutboxEntry, OutboxPush};
pub use stream::{
    Batched, BatchedError, BatchedPoll, DEFAULT_MAX_AGE_US, MAX_MAX_AGE_US, MIN_MAX_AGE_US, Plain,
};
/// The item of a timed batched stream, re-exported for generated code.
pub use ww_si::Timed;

use core::mem::MaybeUninit;
use wire_weaver::shrink_wrap::Error;
#[cfg(not(feature = "std"))]
use wire_weaver::shrink_wrap::series::DeltaPacked;
use wire_weaver::shrink_wrap::series::{
    CodecPolicy, Coder, FloatCodec, NumericCodec, Predictor, SeriesStore,
};

/// Items per batch the scaffold writes.
pub const DEFAULT_BATCH: usize = 32;

/// How batches are encoded unless told otherwise, [DEFAULT_POLICY]'s type: on `std` (hosts, the simulator) the
/// smallest encoding of every column, a run-time [CodecPolicy]; on `no_std` [DeltaPacked](wire_weaver::shrink_wrap::series::DeltaPacked), delta and packed
/// residuals and XOR floats fixed at compile time, so that a firmware links only those codecs (about a third of the
/// CPU of trying every codec and two thirds of the flash, for about 1.2x the bytes, see the batching docs page).
#[cfg(feature = "std")]
pub type DefaultPolicy = CodecPolicy;
#[cfg(not(feature = "std"))]
pub type DefaultPolicy = DeltaPacked;

/// The [DefaultPolicy].
#[cfg(feature = "std")]
pub const DEFAULT_POLICY: DefaultPolicy = CodecPolicy::SMALLEST;
#[cfg(not(feature = "std"))]
pub const DEFAULT_POLICY: DefaultPolicy = DeltaPacked;

/// The codecs of [DEFAULT_POLICY] on `no_std` as a run-time value, for code that picks them on any target (or
/// [DeltaPacked](wire_weaver::shrink_wrap::series::DeltaPacked) as a type).
pub const FIXED_DELTA_PACKED: CodecPolicy = CodecPolicy::fixed(
    NumericCodec::new(Predictor::Delta, Coder::Packed),
    FloatCodec::Xor,
);

/// Why a batch was sealed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cause {
    /// It has `N` items.
    Full,
    /// One more item could make it bigger than the budget.
    Budget,
    /// It got as old as the host allows.
    Age,
    /// [Producer::flush] or [Consumer::flush].
    Flush,
}

/// What [Producer::push] did, or what [Consumer::poll] sealed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seal {
    /// The item went into the filling batch.
    No,
    /// The batch was sealed and waits for the event loop.
    Sealed(Cause),
    /// The batch was full (or due) but the spare store wasn't back from the consumer: its `items` are lost and the
    /// store starts over. The consumer reports them before the store's next batch.
    Lost { cause: Cause, items: usize },
}

/// Counts of a [Batcher] since it was made, for status and to see what batching costs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BatchStats {
    /// Batches the consumer took, by why they were sealed ([Cause]: `Full`, `Budget`, `Age`, `Flush`).
    pub sealed: [u64; 4],
    /// Items reported lost: dropped by the producer while the spare wasn't back, or discarded.
    pub lost: u64,
    /// Items of sealed batches discarded unsent ([Consumer::discard_sealed]), part of `lost`.
    pub discarded: u64,
    /// Events encoded (a split batch is more than one).
    pub encoded: u64,
    /// Items encoded.
    pub encoded_items: u64,
    /// Bytes of the encoded events.
    pub encoded_bytes: u64,
    /// Batches that didn't fit the room they were encoded into and were split.
    pub split: u64,
}

/// An encoded batch, see [Consumer::encode_into].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Encoded {
    /// Items in it: the event's `units`.
    pub items: usize,
    /// Its bytes, from the start of the buffer.
    pub len: usize,
    /// Items lost right before it ([Seal::Lost]): this event's `lost`, to report before its items. Only on the first
    /// part of a split batch.
    pub lost_before: u64,
    /// Why it was sealed.
    pub cause: Cause,
}

/// Why [Consumer::encode_into] wrote nothing; the sealed batch stays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodeError {
    /// Not even one item fits the buffer.
    NoRoom,
    /// The series writer failed (an opaque field that didn't serialize).
    Write(Error),
}

impl From<Error> for EncodeError {
    fn from(e: Error) -> Self {
        EncodeError::Write(e)
    }
}

/// Bytes a [Scratch] needs for batches of `n` items of `T`.
pub const fn scratch_bytes<T>(n: usize) -> usize {
    core::mem::size_of::<T>() * n
}

/// Memory to rebuild one batch's items in while it is encoded or counted, shared by every [Batcher] of a server:
/// as big as the biggest batch, `Scratch<{ scratch_bytes::<Timed<Ampere>>(32) }>`. Rows of a [LevelRow] are
/// rebuilt one column at a time, so for them it is the biggest column: the time (`Second`) or a value. A batcher of `N` items of `T`
/// that doesn't fit is a compile error, so is a `T` aligned to more than 16 bytes.
#[repr(C, align(16))]
pub struct Scratch<const BYTES: usize>(MaybeUninit<[u8; BYTES]>);

impl<const BYTES: usize> Scratch<BYTES> {
    pub const fn new() -> Self {
        Scratch(MaybeUninit::uninit())
    }

    /// Items `range` of `store` rebuilt into the scratch memory.
    fn fill<T: SeriesStore, const N: usize>(
        &mut self,
        store: &T::Store<N>,
        range: core::ops::Range<usize>,
    ) -> Filled<'_, T> {
        const {
            assert!(
                core::mem::align_of::<T>() <= 16,
                "batched item type aligned to more than 16 bytes"
            );
            assert!(
                core::mem::size_of::<T>().saturating_mul(N) <= BYTES,
                "Scratch too small for this batcher: see scratch_bytes"
            );
        }
        // SAFETY: the scratch memory is aligned to 16 bytes (repr(C) puts the array at offset 0), at least as much
        // as `T`, and holds `N` values of `T` (both checked above); `MaybeUninit` needs no initialization, and the
        // slice borrows `self` mutably for its lifetime.
        let slots = unsafe {
            core::slice::from_raw_parts_mut(self.0.as_mut_ptr().cast::<MaybeUninit<T>>(), N)
        };
        let mut filled = Filled { slots, n: 0 };
        for i in range {
            filled.slots[filled.n].write(T::get::<N>(store, i));
            filled.n += 1;
        }
        filled
    }
}

impl<const BYTES: usize> Default for Scratch<BYTES> {
    fn default() -> Self {
        Self::new()
    }
}

/// The first `n` slots hold items, dropped with it.
struct Filled<'s, T> {
    slots: &'s mut [MaybeUninit<T>],
    n: usize,
}

impl<T> Filled<'_, T> {
    fn items(&self) -> &[T] {
        // SAFETY: the first `n` slots were written and not dropped
        unsafe { core::slice::from_raw_parts(self.slots.as_ptr().cast::<T>(), self.n) }
    }
}

impl<T> Drop for Filled<'_, T> {
    fn drop(&mut self) {
        for slot in &mut self.slots[..self.n] {
            // SAFETY: written in `fill`, dropped once here
            unsafe { slot.assume_init_drop() };
        }
    }
}

/// Random pushes, polls, encodes and outbox pushes against a model, for the unit tests and the `batch-seal` fuzz
/// target.
#[cfg(any(test, all(feature = "std", fuzzing)))]
#[doc(hidden)]
pub mod check;
#[cfg(test)]
mod tests;

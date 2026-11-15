//! What a reader walking blocks in order keeps: the current run, the last seq, the layout's gap rules.

use shrink_wrap::prelude::*;

use crate::piece::PieceHeadBlock;
use crate::{Block, BlockKind, Describe, Layout, Medium, RunTag};

/// Whether a good block met in a walk belongs to the data.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// The next block of the current run, or the start of a run that may start there.
    Take,
    /// A block of another run where no run may start: stale data, never taken.
    Stale(RunTag),
    /// A data block of the current run whose seq is not after the last one's.
    Backwards { seq: u64, after: u64 },
}

/// The run and seq of the blocks taken so far, and the gap rules of the newest [Layout] read.
#[derive(Copy, Clone, Debug)]
pub(crate) struct RunState {
    pub(crate) medium: Medium,
    run: Option<RunTag>,
    last_seq: Option<u64>,
    gap_max: u64,
    resume_align: u64,
}

impl RunState {
    pub(crate) fn new(medium: Medium) -> Self {
        RunState {
            medium,
            run: None,
            last_seq: None,
            gap_max: 0,
            resume_align: 1,
        }
    }

    /// Take `layout`'s gap rules (before any describe block says them, for a medium layer that knows them).
    pub(crate) fn set_layout(&mut self, layout: &Layout) {
        self.gap_max = u64::from(layout.gap_max.get());
        self.resume_align = u64::from(layout.resume_align.get()).max(1);
    }

    pub(crate) fn gap_max(&self) -> u64 {
        self.gap_max
    }

    /// What `b` is to the walk. `check_seq` false leaves the seq order out (a salvage walk).
    pub(crate) fn verdict(&self, b: &Block<'_>, check_seq: bool) -> Verdict {
        let Some(run) = self.run else {
            return Verdict::Take;
        };
        if b.run != run {
            return if self.starts_run(b, run) {
                Verdict::Take
            } else {
                Verdict::Stale(b.run)
            };
        }
        let free = crate::index::is_structural(b.kind) || b.kind == BlockKind::Describe;
        match self.last_seq {
            Some(after) if check_seq && !free && b.seq <= after => {
                Verdict::Backwards { seq: b.seq, after }
            }
            _ => Verdict::Take,
        }
    }

    /// Whether `b`, of another run than `current`, may start a run here: on a file a describe block (another
    /// producer run appended) or a piece head (concatenated pieces); on a raw medium only a piece head naming
    /// `current` as the run before it.
    fn starts_run(&self, b: &Block<'_>, current: RunTag) -> bool {
        match self.medium {
            Medium::File => matches!(b.kind, BlockKind::Describe | BlockKind::PieceHead),
            Medium::Raw => {
                b.kind == BlockKind::PieceHead
                    && PieceHeadBlock::read(b).is_ok_and(|h| h.head().prev_run == current)
            }
        }
    }

    /// Note `b`, taken: its run and seq, and the layout its describe records say.
    pub(crate) fn take(&mut self, b: &Block<'_>) {
        if self.run != Some(b.run) {
            self.run = Some(b.run);
            self.last_seq = None;
        }
        if !crate::index::is_structural(b.kind) {
            self.last_seq = Some(b.seq);
        }
        if let Some(l) = layout_in(b) {
            self.set_layout(&l);
        }
    }

    /// Whether a good block found past a gap goes on with the data: the current run's next block (a seq after the
    /// last, or a block that takes no part in the order), or a run that may start there.
    pub(crate) fn resumes(&self, b: &Block<'_>) -> bool {
        self.verdict(b, true) == Verdict::Take
    }

    /// The places past a blank or bad stretch starting at `start` a reader probes for the data going on, ascending,
    /// none at or before `from`: multiples of [Layout::resume_align] within [Layout::gap_max] of `start`. (The
    /// first non-blank byte is the caller's to probe.)
    pub(crate) fn probes(&self, start: u64, from: u64) -> impl Iterator<Item = u64> {
        let align = self.resume_align;
        let end = start.saturating_add(self.gap_max);
        let first = (from.max(start) / align)
            .saturating_add(1)
            .saturating_mul(align);
        (0..)
            .map(move |i: u64| first.saturating_add(i.saturating_mul(align)))
            .take_while(move |p| *p <= end && self.gap_max > 0)
    }
}

/// The [Layout] a describe block says, the last one in it; None for any other block.
pub(crate) fn layout_in(b: &Block<'_>) -> Option<Layout> {
    if b.kind != BlockKind::Describe {
        return None;
    }
    b.records()
        .filter_map(Result::ok)
        .filter(|r| r.track == crate::DESCRIBE_TRACK)
        .filter_map(|r| match Describe::from_ww_bytes(r.item) {
            Ok(Describe::Layout(l)) => Some(l),
            _ => None,
        })
        .last()
}

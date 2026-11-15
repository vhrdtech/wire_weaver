//! What a reader walking blocks in order keeps: the current run, the last seq, the layout's gap rules.

use shrink_wrap::prelude::*;

use crate::piece::PieceHeadBlock;
use crate::{Block, BlockKind, Describe, FLAG_COPY, HEADER_LEN, Layout, Medium, RunTag, block_len};

/// Whether a good block met in a walk belongs to the data.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub(crate) enum Verdict {
    /// The next block of the current run, or the start of a run that may start there.
    Take,
    /// A block of another run where no run may start: stale data, never taken.
    Stale(RunTag),
    /// A data block of the current run whose seq is before the last one's.
    Backwards { seq: u64, after: u64 },
    /// The same (run, seq) as the last block taken: a duplicate, not a cut. `same` when its bytes are the same
    /// (dropped); else the later copy wins.
    Duplicate { same: bool },
}

/// The header fields a walk judges a block by, read before its CRC is checked: what makes a gap probe worth checking
/// whole.
#[derive(Copy, Clone, Debug)]
pub(crate) struct Head {
    /// The whole block's length, as its header says
    pub(crate) len: usize,
    kind: BlockKind,
    run: RunTag,
    seq: u64,
    copy: bool,
}

impl Head {
    /// The header at the start of `bytes`; None when it has no magic or its length is out of range.
    pub(crate) fn read(bytes: &[u8]) -> Option<Head> {
        let len = block_len(bytes).ok()?;
        let h: &[u8; HEADER_LEN] = bytes.get(..HEADER_LEN)?.try_into().ok()?;
        let [_, _, _, _, _, kind, f0, f1, ..] = *h;
        let seq = u64::from_le_bytes(h[16..24].try_into().ok()?);
        let run = u32::from_le_bytes(h[24..28].try_into().ok()?);
        Some(Head {
            len,
            kind: BlockKind::from_u8(kind),
            run: RunTag::from_u32(run),
            seq,
            copy: u16::from_le_bytes([f0, f1]) & FLAG_COPY != 0,
        })
    }
}

/// The run and seq of the blocks taken so far, and the gap rules of the newest [Layout] read.
#[derive(Copy, Clone, Debug)]
pub(crate) struct RunState {
    pub(crate) medium: Medium,
    run: Option<RunTag>,
    last_seq: Option<u64>,
    /// Length and CRC of the last data block taken
    last_bytes: Option<(usize, u32)>,
    gap_max: u64,
    resume_align: u64,
}

impl RunState {
    pub(crate) fn new(medium: Medium) -> Self {
        RunState {
            medium,
            run: None,
            last_seq: None,
            last_bytes: None,
            gap_max: 0,
            resume_align: 1,
        }
    }

    /// Take `layout`'s gap rules (before any describe block says them, for a medium layer that knows them).
    ///
    /// # Panics
    ///
    /// When `layout` is not [valid](Layout::is_valid); a decoded one always is.
    pub(crate) fn set_layout(&mut self, layout: &Layout) {
        assert!(
            layout.is_valid(),
            "a layout's gap rules are within GAP_MAX_MAX and GAP_PROBES_MAX"
        );
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
        if !check_seq || is_free(b) {
            return Verdict::Take;
        }
        match self.last_seq {
            Some(after) if b.seq == after => Verdict::Duplicate {
                same: self.last_bytes == Some((b.len(), b.crc())),
            },
            Some(after) if b.seq < after => Verdict::Backwards { seq: b.seq, after },
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
            self.last_bytes = None;
        }
        if !is_free(b) {
            self.last_seq = Some(b.seq);
            self.last_bytes = Some((b.len(), b.crc()));
        }
        if let Some(l) = layout_in(b) {
            self.set_layout(&l);
        }
    }

    /// Whether a good block found past a gap goes on with the data: the current run's next block (a seq after the
    /// last, or a block that takes no part in the order), the last block again (a rewrite after a failed program,
    /// same bytes or not), or a run that may start there.
    pub(crate) fn resumes(&self, b: &Block<'_>) -> bool {
        matches!(
            self.verdict(b, true),
            Verdict::Take | Verdict::Duplicate { .. }
        )
    }

    /// Whether the block whose header is `h` may go on with the data past a gap: a superset of [Self::resumes], read
    /// from the header alone, so only such a probe is checked whole.
    pub(crate) fn may_resume(&self, h: &Head) -> bool {
        let Some(run) = self.run else {
            return true;
        };
        if h.run != run {
            return match self.medium {
                Medium::File => matches!(h.kind, BlockKind::Describe | BlockKind::PieceHead),
                Medium::Raw => h.kind == BlockKind::PieceHead,
            };
        }
        if crate::index::is_structural(h.kind) || h.copy {
            return true;
        }
        self.last_seq.is_none_or(|after| h.seq >= after)
    }

    /// The places past a blank or bad stretch starting at `start` a reader probes for the data going on, ascending,
    /// none at or before `from`: multiples of [Layout::resume_align] within [Layout::gap_max] of `start`. (The
    /// first non-blank byte is the caller's to probe.)
    /// At most [crate::GAP_PROBES_MAX] + 1, since a valid layout's `gap_max / resume_align` is within it.
    pub(crate) fn probes(&self, start: u64, from: u64) -> impl Iterator<Item = u64> + use<> {
        let (align, gap_max) = (self.resume_align, self.gap_max);
        let end = start.saturating_add(gap_max);
        let first = (from.max(start) / align)
            .saturating_add(1)
            .saturating_mul(align);
        (0..)
            .map(move |i: u64| first.saturating_add(i.saturating_mul(align)))
            .take_while(move |p| *p <= end && gap_max > 0)
    }
}

/// Whether `b` takes no part in the seq order: a block of the file structure (it carries the last counted seq) or a
/// [copy](crate::FLAG_COPY) a writer made.
fn is_free(b: &Block<'_>) -> bool {
    crate::index::is_structural(b.kind) || b.is_copy()
}

/// The [Layout] a describe block says, the last one in it; None for any other block.
pub(crate) fn layout_in(b: &Block<'_>) -> Option<Layout> {
    if b.kind != BlockKind::Describe {
        return None;
    }
    let mut buf = alloc::vec::Vec::new();
    b.decode(&mut buf)
        .ok()?
        .records()
        .filter_map(Result::ok)
        .filter(|r| r.track == crate::DESCRIBE_TRACK)
        .filter_map(|r| match Describe::from_ww_bytes(r.item) {
            Ok(Describe::Layout(l)) => Some(l),
            _ => None,
        })
        .last()
}

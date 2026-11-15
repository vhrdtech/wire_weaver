//! The levels of a summary tree as a writer fills them: open nodes, batches of finished nodes, blocks to encode.

use super::{
    FANOUT_SHIFT, LEVEL_MAX, NODE_SHIFT_MAX, SummaryError, batch_nodes, level_shift, node_span,
};

/// A node that merges with the node right after it.
pub(super) trait LevelNode: Clone {
    /// Merge `next` (`next_items` items) into this node (`items` items).
    fn merge(&mut self, next: &Self, items: u64, next_items: u64) -> Result<(), SummaryError>;
}

/// A batch of consecutive nodes of one level, ready to encode as a block.
pub(super) struct Output<N> {
    pub level: u8,
    pub first_item: u64,
    pub items: u64,
    pub nodes: Vec<N>,
}

struct Level<N> {
    /// The node being merged from its children: number, node, items so far
    open: Option<(u64, N, u64)>,
    /// Finished nodes not yet in a block
    batch: Vec<N>,
    batch_first: u64,
    /// Nodes finished at this level
    total: u64,
}

impl<N> Level<N> {
    const fn new() -> Self {
        Level {
            open: None,
            batch: Vec::new(),
            batch_first: 0,
            total: 0,
        }
    }
}

/// Levels 0 and up of one track's tree.
pub(super) struct Levels<N> {
    origin: u64,
    base_shift: u8,
    /// Nodes per block, a power of two
    batch: u64,
    levels: Vec<Level<N>>,
    out: Vec<Output<N>>,
}

/// A level or node number past what the tree holds: the writer's own bookkeeping, never reached from good input.
const NO_NODE: SummaryError = SummaryError::Invalid("a level or node outside the tree");

impl<N: LevelNode> Levels<N> {
    /// Levels of a tree whose first item is `origin` and whose level-0 nodes hold `2^base_shift` items of
    /// `node_len` bytes in a block of at most `max_block_bytes`.
    pub fn new(origin: u64, base_shift: u8, node_len: usize, max_block_bytes: usize) -> Self {
        Levels {
            origin,
            base_shift,
            batch: batch_nodes(node_len, max_block_bytes),
            levels: Vec::new(),
            out: Vec::new(),
        }
    }

    /// Log2 of the items in a node of `level`.
    pub fn shift(&self, level: u8) -> Result<u8, SummaryError> {
        level_shift(self.base_shift, level).ok_or(NO_NODE)
    }

    /// First item and items of node `idx` of `level` among items `[origin, end)`.
    fn span(&self, level: u8, idx: u64, end: u64) -> Result<(u64, u64), SummaryError> {
        let count = end.checked_sub(self.origin).ok_or(NO_NODE)?;
        node_span(idx, self.shift(level)?, self.origin, count).ok_or(NO_NODE)
    }

    /// Level `level`, added with the levels below it when new.
    fn level_mut(&mut self, level: u8) -> Result<&mut Level<N>, SummaryError> {
        if level > LEVEL_MAX {
            return Err(NO_NODE);
        }
        let l = usize::from(level);
        if self.levels.len() <= l {
            self.levels
                .resize_with(l.checked_add(1).ok_or(NO_NODE)?, Level::new);
        }
        self.levels.get_mut(l).ok_or(NO_NODE)
    }

    /// Hand in node `idx` of `level`, finished (or the last, partial, at the end); `end` is one past the last item
    /// seen. Merges it into its parent and finishes the parent with its last child.
    pub fn push(&mut self, level: u8, idx: u64, node: N, end: u64) -> Result<(), SummaryError> {
        let (_, items) = self.span(level, idx, end)?;
        // a parent only while its nodes hold at most 2^NODE_SHIFT_MAX items
        let parent_level = level
            .checked_add(1)
            .filter(|p| self.shift(*p).is_ok_and(|s| s <= NODE_SHIFT_MAX));
        let batch = self.batch;
        let lv = self.level_mut(level)?;
        if lv.batch.is_empty() {
            lv.batch_first = idx;
        }
        lv.total = lv.total.checked_add(1).ok_or(NO_NODE)?;
        let parent = parent_level.map(|p| (p, node.clone()));
        lv.batch.push(node);
        let next = idx.checked_add(1).ok_or(NO_NODE)?;
        if next.is_multiple_of(batch) {
            self.emit(level, end)?;
        }
        let Some((parent_level, node)) = parent else {
            return Ok(());
        };
        let p = idx >> FANOUT_SHIFT;
        let up = self.level_mut(parent_level)?;
        match &mut up.open {
            Some((pidx, open, n)) => {
                debug_assert_eq!(*pidx, p, "children arrive in order");
                open.merge(&node, *n, items)?;
                *n = n.checked_add(items).ok_or(NO_NODE)?;
            }
            slot @ None => *slot = Some((p, node, items)),
        }
        if next.is_multiple_of(1 << FANOUT_SHIFT)
            && let Some((p, open, _)) = up.open.take()
        {
            self.push(parent_level, p, open, end)?;
        }
        Ok(())
    }

    /// The batch of `level` as a block to encode.
    fn emit(&mut self, level: u8, end: u64) -> Result<(), SummaryError> {
        let lv = self.level_mut(level)?;
        let nodes = core::mem::take(&mut lv.batch);
        let first = lv.batch_first;
        let last = u64::try_from(nodes.len())
            .ok()
            .and_then(|n| n.checked_sub(1))
            .and_then(|n| first.checked_add(n))
            .ok_or(NO_NODE)?;
        let (first_item, _) = self.span(level, first, end)?;
        let (last_from, last_items) = self.span(level, last, end)?;
        let items = last_from
            .checked_add(last_items)
            .and_then(|to| to.checked_sub(first_item))
            .ok_or(NO_NODE)?;
        self.out.push(Output {
            level,
            first_item,
            items,
            nodes,
        });
        Ok(())
    }

    /// Finish every level at `end` (the level-0 node holding the last item already handed in): the open nodes go in
    /// as partial nodes, every batch becomes a block, up to the first level with a single node.
    pub fn finish(&mut self, end: u64) -> Result<(), SummaryError> {
        let mut level = 0u8;
        while usize::from(level) < self.levels.len() {
            if let Some((p, open, _)) = self.level_mut(level)?.open.take() {
                self.push(level, p, open, end)?;
            }
            if !self.level_mut(level)?.batch.is_empty() {
                self.emit(level, end)?;
            }
            if self.level_mut(level)?.total <= 1 {
                break;
            }
            level = level.checked_add(1).ok_or(NO_NODE)?;
        }
        self.levels.truncate(usize::from(level).saturating_add(1));
        for lv in &mut self.levels {
            lv.open = None;
        }
        Ok(())
    }

    /// Blocks to encode, oldest first.
    pub fn take_output(&mut self) -> Vec<Output<N>> {
        core::mem::take(&mut self.out)
    }

    /// Nodes of `level` not in a block yet, then its open node (partial): the first one's number and the nodes.
    pub fn pending(&self, level: u8) -> (u64, Vec<N>) {
        let Some(lv) = self.levels.get(usize::from(level)) else {
            return (0, Vec::new());
        };
        let mut nodes = lv.batch.clone();
        let mut first = lv.batch_first;
        if let Some((p, open, _)) = &lv.open {
            if nodes.is_empty() {
                first = *p;
            }
            nodes.push(open.clone());
        }
        (first, nodes)
    }
}

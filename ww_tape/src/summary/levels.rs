//! The levels of a summary tree as a writer fills them: open nodes, batches of finished nodes, blocks to encode.

use super::{FANOUT_SHIFT, NODE_SHIFT_MAX, SummaryError, batch_nodes};

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
    pub fn shift(&self, level: u8) -> u8 {
        self.base_shift + FANOUT_SHIFT * level
    }

    /// Items of node `idx` of `level` among items `[origin, end)`.
    fn items(&self, level: u8, idx: u64, end: u64) -> u64 {
        let s = self.shift(level);
        let from = (idx << s).max(self.origin);
        let to = ((idx + 1) << s).min(end);
        to - from
    }

    /// Hand in node `idx` of `level`, finished (or the last, partial, at the end); `end` is one past the last item
    /// seen. Merges it into its parent and finishes the parent with its last child.
    pub fn push(&mut self, level: u8, idx: u64, node: N, end: u64) -> Result<(), SummaryError> {
        let l = usize::from(level);
        if self.levels.len() <= l {
            self.levels.resize_with(l + 1, Level::new);
        }
        let items = self.items(level, idx, end);
        let up = self.shift(level) + FANOUT_SHIFT <= NODE_SHIFT_MAX;
        let lv = &mut self.levels[l];
        if lv.batch.is_empty() {
            lv.batch_first = idx;
        }
        lv.total += 1;
        let parent = if up { Some(node.clone()) } else { None };
        lv.batch.push(node);
        if (idx + 1).is_multiple_of(self.batch) {
            self.emit(level, end);
        }
        let Some(node) = parent else { return Ok(()) };
        if self.levels.len() <= l + 1 {
            self.levels.resize_with(l + 2, Level::new);
        }
        let p = idx >> FANOUT_SHIFT;
        match &mut self.levels[l + 1].open {
            Some((pidx, open, n)) => {
                debug_assert_eq!(*pidx, p, "children arrive in order");
                open.merge(&node, *n, items)?;
                *n += items;
            }
            slot @ None => *slot = Some((p, node, items)),
        }
        if (idx + 1).is_multiple_of(1 << FANOUT_SHIFT) {
            let (p, open, _) = self.levels[l + 1].open.take().expect("opened above");
            self.push(level + 1, p, open, end)?;
        }
        Ok(())
    }

    /// The batch of `level` as a block to encode.
    fn emit(&mut self, level: u8, end: u64) {
        let s = self.shift(level);
        let lv = &mut self.levels[usize::from(level)];
        let nodes = core::mem::take(&mut lv.batch);
        let last = lv.batch_first + nodes.len() as u64 - 1;
        let first_item = (lv.batch_first << s).max(self.origin);
        let to = ((last + 1) << s).min(end);
        self.out.push(Output {
            level,
            first_item,
            items: to - first_item,
            nodes,
        });
    }

    /// Finish every level at `end` (the level-0 node holding the last item already handed in): the open nodes go in
    /// as partial nodes, every batch becomes a block, up to the first level with a single node.
    pub fn finish(&mut self, end: u64) -> Result<(), SummaryError> {
        let mut l = 0;
        while l < self.levels.len() {
            let level = l as u8;
            if let Some((p, open, _)) = self.levels[l].open.take() {
                self.push(level, p, open, end)?;
            }
            if !self.levels[l].batch.is_empty() {
                self.emit(level, end);
            }
            if self.levels[l].total <= 1 {
                break;
            }
            l += 1;
        }
        self.levels.truncate(l + 1);
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

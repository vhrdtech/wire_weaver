//! The item tree of an event track as a writer builds it.

use super::levels::{LevelNode, Levels};
use super::{
    EVENT_BASE_SHIFT, EventFields, EventNode, Layout, SummaryError, SummaryHead, Tree, summary_len,
    write_event_summary,
};

impl LevelNode for EventNode {
    fn merge(&mut self, next: &Self, _items: u64, _next_items: u64) {
        EventNode::merge(self, next);
    }
}

/// Default most bytes of an event tree block.
pub const EVENT_BLOCK_BYTES: usize = 4096;

/// A run of consecutive items whose summary is known (a sealed group of the time codec): count, starts, flags OR,
/// latest end.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ItemRun {
    /// Items in the run, 1 and up.
    pub count: u64,
    /// The first item's start.
    pub first_tick: i64,
    /// The last item's start.
    pub last_tick: i64,
    /// OR of the items' flags.
    pub flags: u32,
    /// The largest end of any item, at least `last_tick`.
    pub latest_end: i64,
}

/// Builds the item tree of one event track as items are sealed, and encodes its summary blocks.
///
/// Items go in in item order ([Self::push] one by one, [Self::push_run] a run inside one level-0 node). Finished
/// blocks wait in [Self::take_blocks] for [TapeWriter::defer_block](crate::TapeWriter::defer_block); [Self::finish]
/// writes the partial nodes at the end.
pub struct EventTree {
    track_id: u64,
    track_number: u32,
    fields: EventFields,
    origin: u64,
    next_item: u64,
    /// The open level-0 node
    acc: Option<EventNode>,
    levels: Levels<EventNode>,
    blocks: Vec<Vec<u8>>,
    /// Last start of the last closed level-0 node
    last_tick: Option<i64>,
}

impl EventTree {
    /// The tree of track `track_id` (number `track_number`) from item `first_item` on; `fields` says whether the
    /// track has flags and spans.
    pub fn new(track_id: u64, track_number: u32, fields: EventFields, first_item: u64) -> Self {
        EventTree {
            track_id,
            track_number,
            fields,
            origin: first_item,
            next_item: first_item,
            acc: None,
            levels: Levels::new(
                first_item,
                EVENT_BASE_SHIFT,
                fields.node_len(),
                EVENT_BLOCK_BYTES,
            ),
            blocks: Vec::new(),
            last_tick: None,
        }
    }

    /// Keep blocks within `bytes` (default [EVENT_BLOCK_BYTES]); call before the first item.
    pub fn set_max_block_bytes(&mut self, bytes: usize) {
        self.levels = Levels::new(self.origin, EVENT_BASE_SHIFT, self.fields.node_len(), bytes);
    }

    /// The next item's number.
    pub fn next_item(&self) -> u64 {
        self.next_item
    }

    /// Add the next item: its start, end (the start for an instant) and flags. Items come in order of start.
    pub fn push(&mut self, start: i64, end: i64, flags: u32) -> Result<(), SummaryError> {
        self.push_run(ItemRun {
            count: 1,
            first_tick: start,
            last_tick: start,
            flags,
            latest_end: end.max(start),
        })
    }

    /// Add the next `run.count` items at once; the run must lie inside one level-0 node (2048 items).
    pub fn push_run(&mut self, run: ItemRun) -> Result<(), SummaryError> {
        if run.count == 0 {
            return Err(SummaryError::Invalid("a run of no items"));
        }
        if run.last_tick < run.first_tick || run.latest_end < run.last_tick {
            return Err(SummaryError::Invalid("run ticks out of order"));
        }
        if !self.fields.latest_end && run.latest_end != run.last_tick {
            return Err(SummaryError::Invalid("a span in a tree of instants"));
        }
        let shift = EVENT_BASE_SHIFT;
        let end = self
            .next_item
            .checked_add(run.count)
            .ok_or(SummaryError::Invalid("item numbers past u64"))?;
        if (self.next_item >> shift) != ((end - 1) >> shift) {
            return Err(SummaryError::Invalid("a run across a level-0 node"));
        }
        let node = EventNode {
            first_tick: run.first_tick,
            last_tick: run.last_tick,
            flags: if self.fields.flags { run.flags } else { 0 },
            latest_end: run.latest_end,
        };
        match &mut self.acc {
            Some(acc) => {
                if run.first_tick < acc.last_tick {
                    return Err(SummaryError::Invalid("items out of order of start"));
                }
                acc.merge(&node);
            }
            None => {
                if let Some(prev) = self.last_tick
                    && run.first_tick < prev
                {
                    return Err(SummaryError::Invalid("items out of order of start"));
                }
                self.acc = Some(node);
            }
        }
        self.next_item = end;
        if end.is_multiple_of(1 << shift) {
            self.close_node();
            self.encode()?;
        }
        Ok(())
    }

    /// Hand the open level-0 node up.
    fn close_node(&mut self) {
        let acc = self.acc.take().expect("a node is open");
        self.last_tick = Some(acc.last_tick);
        let idx = (self.next_item - 1) >> EVENT_BASE_SHIFT;
        self.levels.push(0, idx, acc, self.next_item);
    }

    /// Write the open nodes of every level, partial ones included, as blocks. Call it once, after the last item.
    pub fn finish(&mut self) -> Result<(), SummaryError> {
        if self.acc.is_some() {
            self.close_node();
        }
        self.levels.finish(self.next_item);
        self.encode()
    }

    fn encode(&mut self) -> Result<(), SummaryError> {
        for out in self.levels.take_output() {
            let first = out.nodes.first().expect("a batch holds nodes");
            let last = out.nodes.last().expect("a batch holds nodes");
            let head = SummaryHead {
                track_id: self.track_id,
                track_number: self.track_number,
                tree: Tree::Events,
                level: out.level,
                node_shift: self.levels.shift(out.level),
                layout: Layout::Events(self.fields),
                first_item: out.first_item,
                items: out.items,
                first_tick: first.first_tick,
                last_tick: last.last_tick,
                latest_end: out
                    .nodes
                    .iter()
                    .map(|n| n.latest_end)
                    .max()
                    .expect("a batch holds nodes"),
            };
            let mut buf = vec![0u8; summary_len(&head)];
            let len = write_event_summary(&mut buf, 0, &head, &out.nodes)?;
            buf.truncate(len);
            self.blocks.push(buf);
        }
        Ok(())
    }

    /// Finished summary blocks, oldest first, for [TapeWriter::defer_block](crate::TapeWriter::defer_block).
    pub fn take_blocks(&mut self) -> Vec<Vec<u8>> {
        core::mem::take(&mut self.blocks)
    }

    /// What a live reader takes from memory for `level`: the number of the first node not in a block yet, and the
    /// nodes from it on; the last may be partial (the open node).
    pub fn pending(&self, level: u8) -> (u64, Vec<EventNode>) {
        let (mut first, mut nodes) = self.levels.pending(level);
        if level == 0
            && let Some(acc) = self.acc
        {
            if nodes.is_empty() {
                first = (self.next_item - 1) >> EVENT_BASE_SHIFT;
            }
            nodes.push(acc);
        }
        (first, nodes)
    }
}

//! Summary blocks: the stored levels of a track's summary tree, read in place.
//!
//! A track's items (an event track's items, a series' samples) are grouped into **nodes** over item numbers, not over
//! time: a node at level 0 holds `2^shift` items, a node one level up the nodes of 16 nodes below. Node `n` of a level
//! covers items `[n << shift, (n + 1) << shift)` of the track, numbered from the track's first item, so nodes of two
//! tape pieces of one track line up, and a piece's first and last node may hold fewer items (a *partial* node).
//!
//! - **Event item tree** ([Tree::Events]): nodes of 2048 items at level 0. A node holds the first item's start, the
//!   last item's start, the OR of the items' flags and, for spans, the latest end of any item. Counts are implicit.
//!   An overview of a track reads only the level whose nodes are about as many as the pixels, never the items.
//! - **Series summaries** ([Tree::Series]): nodes of `2^base_shift` samples at level 0 ([SeriesSummarySpec]). A node
//!   holds per channel the statistics the track declares ([SeriesStats]); no ticks, since a series' sample ticks
//!   follow from its segments' grids and period.
//!
//! Every statistic merges: a parent is its 16 children merged ([EventNode::merge], [ChannelStats::merge]), in item
//! order, so a reader building coarser bins (a view cache) gets what the writer would have written.
//!
//! # Scales
//!
//! A series segment may change the scale of the stored numbers (an autoranging device changes prefix), and nodes run
//! over sample indexes, so a node may hold samples of two scales. Such a node is kept at their common scale
//! ([common_scale]: the largest scale both are whole multiples of, `10⁻⁶` for `10⁻³` and `10⁻⁶`): every number of the
//! coarser samples is multiplied by a whole factor, so minimum and maximum stay exact integers and sums don't mix
//! scales ([ChannelStats::rescaled], [SeriesNode::merge]). A summary block states its scale: the common scale of
//! all its nodes, which it stores at that scale, so a block is read alone, without the segment records. Merging nodes
//! of two blocks (a view cache across a change of scale) goes through [SeriesNode::merge], which brings both to their
//! common scale first; two scales with no common scale (a ratio that isn't a fraction of integers) are refused, never
//! mixed.
//!
//! A summary block ([BlockKind::Summary]) holds a batch of consecutive nodes of one level of one track. The block
//! header's count (bytes 12..16) is the number of nodes; bytes 24..32 stay reserved, zero:
//!
//! ```text
//! off size field
//!  32 u64  track id (TrackDef::id)
//!  40 u32  track number (TrackDef::number)
//!  44  u8  tree: 1 events, 2 series (Tree)
//!  45  u8  level, 0 and up
//!  46  u8  node shift: a node holds 2^shift items (events: 11 + 4 · level; series: base_shift + 4 · level)
//!  47  u8  element of a series' samples (Element code), 0 for events
//!  48 u16  fields: the statistics stored (EventFields, SeriesStats bits)
//!  50 u16  channels of a series, 1 and up; 0 for events
//!  52 u16  scale length: bytes of the scale after the head, 1 to SCALE_LEN_MAX for a series, 0 for events
//!  54 u16  reserved, 0
//!  56 u64  first item: the first item the block covers (the first node's first item, or later in a partial node)
//!  64 u64  items the block covers, 1 and up; the nodes are those holding items [first, first + items)
//!  72 i64  first tick: the first covered item's start
//!  80 i64  last tick: the last covered item's start, at least the first tick
//!  88 i64  latest end: the largest end of a covered item, at least the last tick
//!  96 ...  scale of a series: what a stored minimum, maximum or sum is worth in the sample type's unit, as the
//!          shrink_wrap bytes of an exact_units Magnitude (10⁻³ is fa 00); squares and powers are at its square
//! 96+s ... body: columns of n values each, n the node count, little-endian, back to back
//! len-4 u32 CRC over bytes 0..len-4, as in every block
//! ```
//!
//! Event body: first ticks (i64), last ticks (i64), then flags ORs (u32) when [EventFields::flags], then latest ends
//! (i64) when [EventFields::latest_end].
//!
//! Series body: for each statistic the block holds, in the order of [Stat::ALL], for each channel, for each component
//! of a sample (2 for a complex element's minimum, maximum, sum and sum of squares, else 1), one column. Value widths:
//! minimum and maximum 8 bytes (i64, u64 or f64, by the element's kind: [Value]), sum and sum of squares f64, clipped
//! count u64, flags u32, mean and maximum power f32.
//!
//! Summary blocks are small (a few KiB) and rebuilt from the data when lost. In an aligned file the writer places them
//! in the gap before the next aligned chunk payload ([TapeWriter::defer_block](crate::TapeWriter::defer_block)).

use exact_units::Magnitude;
use shrink_wrap::prelude::*;

use crate::{BlockKind, CRC_LEN, HEADER_LEN, MAGIC, MAX_BLOCK_LEN, SeriesError, VERSION};

mod series;
use crate::Element;
#[cfg(feature = "alloc")]
pub use series::SeriesNode;
pub use series::{
    ChannelStats, ClipRange, FACTOR_MAX, ScaleError, SeriesStats, SeriesSummarySpec, Stat, Value,
    common_scale, scale_factor,
};
#[cfg(feature = "std")]
pub use series::{SERIES_BLOCK_BYTES, Scalar, SeriesStart, SeriesSummaries};
use series::{components, is_complex};

#[cfg(feature = "std")]
mod levels;

#[cfg(feature = "std")]
mod tree;
#[cfg(feature = "std")]
pub use tree::{EVENT_BLOCK_BYTES, EventTree, ItemRun};

/// Bytes of the summary head after the block header (32..96).
pub const SUMMARY_FIXED_LEN: usize = 64;
/// Node shift of the event tree at level 0: 2048 items.
pub const EVENT_BASE_SHIFT: u8 = 11;
/// Log2 of the nodes of one level merged into one node of the level above: 16.
pub const FANOUT_SHIFT: u8 = 4;
/// Largest node shift: a node holds at most 2^62 items.
pub const NODE_SHIFT_MAX: u8 = 62;
/// Most bytes of a series block's scale: a power of 10 takes 2, one with two more primes about 10.
pub const SCALE_LEN_MAX: usize = 64;

/// Which kind of tree a summary block belongs to.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum Tree {
    /// The item tree of an event track.
    Events,
    /// The summaries of a fixed-rate series.
    Series,
}

impl Tree {
    fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(Tree::Events),
            2 => Some(Tree::Series),
            _ => None,
        }
    }

    pub(crate) fn to_u8(self) -> u8 {
        match self {
            Tree::Events => 1,
            Tree::Series => 2,
        }
    }
}

/// What an event tree stores besides the first and last tick of each node.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct EventFields {
    /// The OR of the items' flags words: the track has a flags column.
    pub flags: bool,
    /// The latest end of any item: the track holds spans.
    pub latest_end: bool,
}

impl EventFields {
    const FLAGS: u16 = 1;
    const LATEST_END: u16 = 2;

    fn bits(self) -> u16 {
        (if self.flags { Self::FLAGS } else { 0 })
            | (if self.latest_end { Self::LATEST_END } else { 0 })
    }

    fn from_bits(v: u16) -> Option<Self> {
        if v & !(Self::FLAGS | Self::LATEST_END) != 0 {
            return None;
        }
        Some(EventFields {
            flags: v & Self::FLAGS != 0,
            latest_end: v & Self::LATEST_END != 0,
        })
    }

    /// Bytes of one node.
    fn node_len(self) -> usize {
        16 + if self.flags { 4 } else { 0 } + if self.latest_end { 8 } else { 0 }
    }
}

/// What a summary block covers: the fixed part of its head.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct SummaryHead {
    /// [TrackDef::id](crate::TrackDef::id).
    pub track_id: u64,
    /// [TrackDef::number](crate::TrackDef::number).
    pub track_number: u32,
    pub tree: Tree,
    pub level: u8,
    /// A node holds `2^node_shift` items.
    pub node_shift: u8,
    /// What the block stores per node.
    pub layout: Layout,
    /// The first item the block covers.
    pub first_item: u64,
    /// Items the block covers, 1 and up.
    pub items: u64,
    /// The first covered item's start.
    pub first_tick: i64,
    /// The last covered item's start.
    pub last_tick: i64,
    /// The largest end of a covered item; the last tick for instants and series.
    pub latest_end: i64,
}

/// What a summary block stores per node, by its tree.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "a head is read once per block; a boxed scale would cost Copy"
)]
pub enum Layout {
    Events(EventFields),
    Series {
        element: Element,
        channels: u16,
        stats: SeriesStats,
        /// What a stored minimum, maximum or sum of every node in the block is worth in the sample type's unit.
        scale: Magnitude,
    },
}

impl SummaryHead {
    /// Number of the first node: the node holding [Self::first_item].
    pub fn first_node(&self) -> u64 {
        self.first_item >> self.node_shift
    }

    /// Nodes in the block, 1 and up.
    pub fn node_count(&self) -> u64 {
        let last = (self.first_item + (self.items - 1)) >> self.node_shift;
        last - self.first_node() + 1
    }

    /// The items of node `i` of the block (0 is [Self::first_node]): first item and count, fewer than `2^shift` in a
    /// partial node at either end.
    ///
    /// # Panics
    ///
    /// When `i` is not below [Self::node_count].
    pub fn node_items(&self, i: u64) -> (u64, u64) {
        assert!(i < self.node_count(), "node {i} of {}", self.node_count());
        let n = self.first_node() + i;
        let from = (n << self.node_shift).max(self.first_item);
        let to = ((n + 1) << self.node_shift).min(self.first_item + self.items);
        (from, to - from)
    }

    /// Bytes of one node in the body.
    fn node_len(&self) -> usize {
        match self.layout {
            Layout::Events(f) => f.node_len(),
            Layout::Series {
                element,
                channels,
                stats,
                ..
            } => stats.node_len(element) * usize::from(channels),
        }
    }

    /// Check what a head can say on its own.
    fn check(&self) -> Result<(), SummaryError> {
        if self.items == 0 {
            return Err(SummaryError::Invalid("no items"));
        }
        if self.first_item.checked_add(self.items).is_none() {
            return Err(SummaryError::Invalid("item numbers past u64"));
        }
        if self.node_shift > NODE_SHIFT_MAX {
            return Err(SummaryError::Invalid("node shift past 62"));
        }
        if self.last_tick < self.first_tick || self.latest_end < self.last_tick {
            return Err(SummaryError::Invalid(
                "ticks out of order: first, last, latest end",
            ));
        }
        match (self.tree, self.layout) {
            (Tree::Events, Layout::Events(f)) => {
                let want =
                    u32::from(EVENT_BASE_SHIFT) + u32::from(FANOUT_SHIFT) * u32::from(self.level);
                if u32::from(self.node_shift) != want {
                    return Err(SummaryError::Invalid("event node shift not 11 + 4 · level"));
                }
                if !f.latest_end && self.latest_end != self.last_tick {
                    return Err(SummaryError::Invalid(
                        "latest end of instants not the last tick",
                    ));
                }
            }
            (
                Tree::Series,
                Layout::Series {
                    element,
                    channels,
                    stats,
                    ..
                },
            ) => {
                if u32::from(self.node_shift) < u32::from(FANOUT_SHIFT) * u32::from(self.level) {
                    return Err(SummaryError::Invalid("series node shift below 4 · level"));
                }
                if channels == 0 {
                    return Err(SummaryError::Invalid("a series has no channels"));
                }
                if stats.is_empty() {
                    return Err(SummaryError::Invalid("no statistics"));
                }
                if (stats.power_mean || stats.power_max) && !is_complex(element) {
                    return Err(SummaryError::Invalid("power of a real element"));
                }
                if self.latest_end != self.last_tick {
                    return Err(SummaryError::Invalid(
                        "latest end of a series not the last tick",
                    ));
                }
            }
            _ => return Err(SummaryError::Invalid("layout of another tree")),
        }
        Ok(())
    }
}

/// Why a summary block can't be written or read.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SummaryError {
    /// The buffer is shorter than the block ([summary_len]).
    Full { needed: usize },
    /// The block would be longer than [MAX_BLOCK_LEN].
    TooLong { len: usize },
    /// The head breaks a rule of the format, or the body is not as long as the head says.
    Invalid(&'static str),
    /// A series block's scale doesn't encode or decode.
    ScaleBytes(shrink_wrap::Error),
    /// Statistics at two scales can't be brought to one.
    Scale(ScaleError),
    /// A segment a series writer was handed breaks a rule of the series' timeline.
    Series(SeriesError),
}

/// The scale bytes of a block with `head`: empty for an event tree.
fn scale_bytes<'a>(
    head: &SummaryHead,
    buf: &'a mut [u8; SCALE_LEN_MAX],
) -> Result<&'a [u8], SummaryError> {
    match head.layout {
        Layout::Events(_) => Ok(&[]),
        Layout::Series { scale, .. } => scale.to_ww_bytes(buf).map_err(SummaryError::ScaleBytes),
    }
}

/// Bytes of a summary block with `head`.
pub fn summary_len(head: &SummaryHead) -> Result<usize, SummaryError> {
    let mut scale = [0u8; SCALE_LEN_MAX];
    let scale_len = scale_bytes(head, &mut scale)?.len();
    usize::try_from(head.node_count())
        .ok()
        .and_then(|n| n.checked_mul(head.node_len()))
        .and_then(|n| n.checked_add(HEADER_LEN + SUMMARY_FIXED_LEN + scale_len + CRC_LEN))
        .ok_or(SummaryError::TooLong { len: usize::MAX })
}

fn put_u16(buf: &mut [u8], at: usize, v: u16) {
    buf[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

fn put_u32(buf: &mut [u8], at: usize, v: u32) {
    buf[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

fn put_u64(buf: &mut [u8], at: usize, v: u64) {
    buf[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

/// The `N` bytes at `at`, None past the end.
fn get<const N: usize>(buf: &[u8], at: usize) -> Option<[u8; N]> {
    buf.get(at..at.checked_add(N)?)?.try_into().ok()
}

fn get_u8(buf: &[u8], at: usize) -> Option<u8> {
    buf.get(at).copied()
}

fn get_u16(buf: &[u8], at: usize) -> Option<u16> {
    get(buf, at).map(u16::from_le_bytes)
}

fn get_u32(buf: &[u8], at: usize) -> Option<u32> {
    get(buf, at).map(u32::from_le_bytes)
}

fn get_u64(buf: &[u8], at: usize) -> Option<u64> {
    get(buf, at).map(u64::from_le_bytes)
}

/// Writes one summary block into a buffer: the head at [Self::new], the columns ([Self::body_mut]), the CRC at
/// [Self::finish].
pub struct SummaryWriter<'b> {
    buf: &'b mut [u8],
    nodes: usize,
    /// Where the body starts: after the head and the scale
    body_at: usize,
}

impl<'b> SummaryWriter<'b> {
    /// Write the header and head of a summary block with `head`, its body zero until filled in.
    pub fn new(buf: &'b mut [u8], seq: u64, head: &SummaryHead) -> Result<Self, SummaryError> {
        head.check()?;
        let len = summary_len(head)?;
        if len > MAX_BLOCK_LEN {
            return Err(SummaryError::TooLong { len });
        }
        if buf.len() < len {
            return Err(SummaryError::Full { needed: len });
        }
        let nodes = head.node_count() as usize;
        let buf = &mut buf[..len];
        buf.fill(0);
        buf[0..4].copy_from_slice(&MAGIC);
        buf[4] = VERSION;
        buf[5] = BlockKind::Summary.to_u8();
        put_u32(buf, 8, len as u32);
        put_u32(buf, 12, nodes as u32);
        put_u64(buf, 16, seq);
        put_u64(buf, 32, head.track_id);
        put_u32(buf, 40, head.track_number);
        buf[44] = head.tree.to_u8();
        buf[45] = head.level;
        buf[46] = head.node_shift;
        let (element, fields, channels) = match head.layout {
            Layout::Events(f) => (0, f.bits(), 0),
            Layout::Series {
                element,
                channels,
                stats,
                ..
            } => (element.code(), stats.bits(), channels),
        };
        buf[47] = element;
        put_u16(buf, 48, fields);
        put_u16(buf, 50, channels);
        let mut scale = [0u8; SCALE_LEN_MAX];
        let scale = scale_bytes(head, &mut scale)?;
        put_u16(buf, 52, scale.len() as u16);
        let at = HEADER_LEN + SUMMARY_FIXED_LEN;
        let body_at = at + scale.len();
        buf[at..body_at].copy_from_slice(scale);
        put_u64(buf, 56, head.first_item);
        put_u64(buf, 64, head.items);
        put_u64(buf, 72, head.first_tick as u64);
        put_u64(buf, 80, head.last_tick as u64);
        put_u64(buf, 88, head.latest_end as u64);
        Ok(SummaryWriter {
            buf,
            nodes,
            body_at,
        })
    }

    /// The body: the columns back to back, [Summary::body] order.
    pub fn body_mut(&mut self) -> &mut [u8] {
        let end = self.buf.len() - CRC_LEN;
        &mut self.buf[self.body_at..end]
    }

    /// Nodes in the block.
    pub fn nodes(&self) -> usize {
        self.nodes
    }

    /// Write the CRC; returns the block's length.
    pub fn finish(self) -> usize {
        let len = self.buf.len();
        let crc = crate::CRC.checksum(&self.buf[..len - CRC_LEN]);
        put_u32(self.buf, len - CRC_LEN, crc);
        len
    }
}

/// The summary of a summary block, read in place.
#[derive(Copy, Clone, Debug)]
pub struct Summary<'b> {
    head: SummaryHead,
    body: &'b [u8],
    nodes: usize,
}

impl<'b> Summary<'b> {
    /// Read and check the head of the summary block `block` (exactly its bytes, CRC checked by the caller).
    pub(crate) fn read(block: &'b [u8]) -> Result<Self, SummaryError> {
        let short = SummaryError::Invalid("shorter than the head");
        let len = block.len();
        let body_end = len.checked_sub(CRC_LEN).ok_or(short)?;
        if body_end < HEADER_LEN + SUMMARY_FIXED_LEN {
            return Err(short);
        }
        let u8_at = |at| get_u8(block, at).ok_or(short);
        let u16_at = |at| get_u16(block, at).ok_or(short);
        let u32_at = |at| get_u32(block, at).ok_or(short);
        let u64_at = |at| get_u64(block, at).ok_or(short);
        if u64_at(24)? != 0 || u16_at(54)? != 0 {
            return Err(SummaryError::Invalid("reserved bytes not zero"));
        }
        let tree = Tree::from_u8(u8_at(44)?).ok_or(SummaryError::Invalid("unknown tree"))?;
        let fields = u16_at(48)?;
        let channels = u16_at(50)?;
        let scale_len = usize::from(u16_at(52)?);
        let scale_at = HEADER_LEN + SUMMARY_FIXED_LEN;
        let body_at = scale_at + scale_len;
        let scale_bytes = block
            .get(scale_at..body_at.min(body_end))
            .filter(|b| b.len() == scale_len)
            .ok_or(SummaryError::Invalid("scale past the end"))?;
        let layout = match tree {
            Tree::Events => {
                if u8_at(47)? != 0 || channels != 0 || scale_len != 0 {
                    return Err(SummaryError::Invalid(
                        "element, channels or scale in an event tree",
                    ));
                }
                Layout::Events(
                    EventFields::from_bits(fields)
                        .ok_or(SummaryError::Invalid("unknown fields"))?,
                )
            }
            Tree::Series => Layout::Series {
                element: Element::from_code(u8_at(47)?)
                    .ok_or(SummaryError::Invalid("unknown element"))?,
                channels,
                stats: SeriesStats::from_bits(fields)
                    .ok_or(SummaryError::Invalid("unknown statistics"))?,
                scale: read_scale(scale_bytes)?,
            },
        };
        let head = SummaryHead {
            track_id: u64_at(32)?,
            track_number: u32_at(40)?,
            tree,
            level: u8_at(45)?,
            node_shift: u8_at(46)?,
            layout,
            first_item: u64_at(56)?,
            items: u64_at(64)?,
            first_tick: u64_at(72)? as i64,
            last_tick: u64_at(80)? as i64,
            latest_end: u64_at(88)? as i64,
        };
        head.check()?;
        let nodes = head.node_count();
        if nodes != u64::from(u32_at(12)?) {
            return Err(SummaryError::Invalid(
                "node count differs from the head's items",
            ));
        }
        let body = block
            .get(body_at..body_end)
            .ok_or(SummaryError::Invalid("body length differs from the head"))?;
        let nodes = usize::try_from(nodes)
            .ok()
            .ok_or(SummaryError::Invalid("more nodes than fit in memory"))?;
        if nodes.checked_mul(head.node_len()) != Some(body.len()) {
            return Err(SummaryError::Invalid("body length differs from the head"));
        }
        Ok(Summary { head, body, nodes })
    }

    pub fn head(&self) -> SummaryHead {
        self.head
    }

    /// The scale of a series block's numbers, None in an event tree.
    pub fn scale(&self) -> Option<Magnitude> {
        match self.head.layout {
            Layout::Series { scale, .. } => Some(scale),
            Layout::Events(_) => None,
        }
    }

    /// Nodes in the block.
    pub fn node_count(&self) -> usize {
        self.nodes
    }

    /// The body, columns back to back.
    pub fn body(&self) -> &'b [u8] {
        self.body
    }

    /// The `N` bytes of value `i` of the column at byte `column_at`, None past the body.
    fn word<const N: usize>(&self, column_at: usize, i: usize) -> Option<[u8; N]> {
        get(self.body, column_at.checked_add(i.checked_mul(N)?)?)
    }

    /// Node `i` of an event tree (0 is the first node); None past the last or in a series.
    pub fn event_node(&self, i: usize) -> Option<EventNode> {
        let Layout::Events(f) = self.head.layout else {
            return None;
        };
        if i >= self.nodes {
            return None;
        }
        let n = self.nodes;
        let first_tick = i64::from_le_bytes(self.word(0, i)?);
        let last_tick = i64::from_le_bytes(self.word(8 * n, i)?);
        let mut at = 16 * n;
        let flags = if f.flags {
            let v = u32::from_le_bytes(self.word(at, i)?);
            at += 4 * n;
            v
        } else {
            0
        };
        let latest_end = if f.latest_end {
            i64::from_le_bytes(self.word(at, i)?)
        } else {
            last_tick
        };
        Some(EventNode {
            first_tick,
            last_tick,
            flags,
            latest_end,
        })
    }

    /// The column of `stat` for `channel` and `component` of a series, at the block's [scale](Self::scale); None
    /// when the block doesn't hold it.
    pub fn series_column(&self, stat: Stat, channel: u16, component: u8) -> Option<StatColumn<'b>> {
        let Layout::Series {
            element,
            channels,
            stats,
            ..
        } = self.head.layout
        else {
            return None;
        };
        if !stats.has(stat) || channel >= channels {
            return None;
        }
        let components = stat.components(element);
        if component >= components {
            return None;
        }
        let mut at = 0usize;
        for s in Stat::ALL {
            if s == stat {
                break;
            }
            if stats.has(s) {
                at += s.width() * usize::from(s.components(element)) * usize::from(channels);
            }
        }
        let width = stat.width();
        let col = usize::from(channel) * usize::from(components) + usize::from(component);
        let from = at
            .checked_add(col.checked_mul(width)?)?
            .checked_mul(self.nodes)?;
        let bytes = self
            .body
            .get(from..from.checked_add(width.checked_mul(self.nodes)?)?)?;
        Some(StatColumn {
            stat,
            element,
            bytes,
        })
    }

    /// The statistics of `channel` in node `i` of a series, at the block's [scale](Self::scale); None past the last
    /// node or channel, or in an event tree. Statistics the block doesn't hold read as their [ChannelStats::empty]
    /// values.
    pub fn channel_stats(&self, channel: u16, i: usize) -> Option<ChannelStats> {
        let Layout::Series {
            element, channels, ..
        } = self.head.layout
        else {
            return None;
        };
        if i >= self.nodes || channel >= channels {
            return None;
        }
        let mut s = ChannelStats::empty(element);
        let get = |stat, c| {
            self.series_column(stat, channel, c)
                .and_then(|col| col.get(i))
        };
        for c in 0..components(element) {
            let k = usize::from(c);
            if let Some(v) = get(Stat::Min, c) {
                s.min[k] = v;
            }
            if let Some(v) = get(Stat::Max, c) {
                s.max[k] = v;
            }
            if let Some(Value::F64(v)) = get(Stat::Sum, c) {
                s.sum[k] = v;
            }
            if let Some(Value::F64(v)) = get(Stat::SumSq, c) {
                s.sum_sq[k] = v;
            }
        }
        if let Some(Value::U64(v)) = get(Stat::Clipped, 0) {
            s.clipped = v;
        }
        if let Some(Value::U64(v)) = get(Stat::Flags, 0) {
            s.flags = v as u32;
        }
        if let Some(Value::F64(v)) = get(Stat::PowerMean, 0) {
            s.power_mean = v as f32;
        }
        if let Some(Value::F64(v)) = get(Stat::PowerMax, 0) {
            s.power_max = v as f32;
        }
        Some(s)
    }

    /// Node `i` of a series, every channel, at the block's scale; None past the last node or in an event tree.
    #[cfg(feature = "alloc")]
    pub fn series_node(&self, i: usize) -> Option<SeriesNode> {
        let Layout::Series {
            channels, scale, ..
        } = self.head.layout
        else {
            return None;
        };
        let channels = (0..channels)
            .map(|ch| self.channel_stats(ch, i))
            .collect::<Option<_>>()?;
        Some(SeriesNode { scale, channels })
    }
}

/// The scale of a series block: a Magnitude that takes exactly its bytes, written the one way it is written.
fn read_scale(bytes: &[u8]) -> Result<Magnitude, SummaryError> {
    if bytes.is_empty() {
        return Err(SummaryError::Invalid("a series block without a scale"));
    }
    let scale = Magnitude::from_ww_bytes(bytes).map_err(SummaryError::ScaleBytes)?;
    let mut again = [0u8; SCALE_LEN_MAX];
    if scale
        .to_ww_bytes(&mut again)
        .map_err(SummaryError::ScaleBytes)?
        != bytes
    {
        return Err(SummaryError::Invalid(
            "scale bytes not as the scale writes them",
        ));
    }
    Ok(scale)
}

/// One statistic of one channel and component over the nodes of a summary block.
#[derive(Copy, Clone, Debug)]
pub struct StatColumn<'b> {
    stat: Stat,
    element: Element,
    bytes: &'b [u8],
}

impl StatColumn<'_> {
    /// Values in the column: the block's node count.
    pub fn len(&self) -> usize {
        self.bytes.len() / self.stat.width()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// The value of node `i`, None past the last: minimum and maximum by the element's kind, sums and powers as
    /// [Value::F64], counts and flags as [Value::U64].
    pub fn get(&self, i: usize) -> Option<Value> {
        let at = i.checked_mul(self.stat.width())?;
        Some(match self.stat {
            Stat::Min | Stat::Max => Value::from_bits(self.element, get_u64(self.bytes, at)?),
            Stat::Sum | Stat::SumSq => Value::F64(f64::from_bits(get_u64(self.bytes, at)?)),
            Stat::Clipped => Value::U64(get_u64(self.bytes, at)?),
            Stat::Flags => Value::U64(u64::from(get_u32(self.bytes, at)?)),
            Stat::PowerMean | Stat::PowerMax => {
                Value::F64(f64::from(f32::from_bits(get_u32(self.bytes, at)?)))
            }
        })
    }
}

/// A node of an event tree, or the merge of consecutive nodes.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct EventNode {
    /// Start of the node's first item.
    pub first_tick: i64,
    /// Start of the node's last item.
    pub last_tick: i64,
    /// OR of the items' flags; 0 when the track has none.
    pub flags: u32,
    /// The largest end of any item; the last tick when every item is an instant.
    pub latest_end: i64,
}

impl EventNode {
    /// A node of one item.
    pub const fn item(start: i64, end: i64, flags: u32) -> Self {
        EventNode {
            first_tick: start,
            last_tick: start,
            flags,
            latest_end: if end > start { end } else { start },
        }
    }

    /// Merge `next`, the node right after this one, into this one.
    pub fn merge(&mut self, next: &EventNode) {
        self.last_tick = next.last_tick;
        self.flags |= next.flags;
        self.latest_end = self.latest_end.max(next.latest_end);
    }
}

/// Encode an event tree block of `nodes` (one per node of `head`, in order) into `buf`; returns its length.
pub fn write_event_summary(
    buf: &mut [u8],
    seq: u64,
    head: &SummaryHead,
    nodes: &[EventNode],
) -> Result<usize, SummaryError> {
    let Layout::Events(f) = head.layout else {
        return Err(SummaryError::Invalid("not an event tree"));
    };
    let mut w = SummaryWriter::new(buf, seq, head)?;
    let n = w.nodes();
    if nodes.len() != n {
        return Err(SummaryError::Invalid("node count differs from the head"));
    }
    let body = w.body_mut();
    let mut at = 0;
    for v in nodes.iter().map(|x| x.first_tick) {
        body[at..at + 8].copy_from_slice(&v.to_le_bytes());
        at += 8;
    }
    for v in nodes.iter().map(|x| x.last_tick) {
        body[at..at + 8].copy_from_slice(&v.to_le_bytes());
        at += 8;
    }
    if f.flags {
        for v in nodes.iter().map(|x| x.flags) {
            body[at..at + 4].copy_from_slice(&v.to_le_bytes());
            at += 4;
        }
    }
    if f.latest_end {
        for v in nodes.iter().map(|x| x.latest_end) {
            body[at..at + 8].copy_from_slice(&v.to_le_bytes());
            at += 8;
        }
    }
    debug_assert_eq!(at, body.len(), "every body byte written");
    Ok(w.finish())
}

/// Encode a series summary block into `buf`: `nodes[i][ch]` is channel `ch` of node `i` of `head`; returns its
/// length.
pub fn write_series_summary(
    buf: &mut [u8],
    seq: u64,
    head: &SummaryHead,
    nodes: &[&[ChannelStats]],
) -> Result<usize, SummaryError> {
    let Layout::Series {
        element,
        channels,
        stats,
        ..
    } = head.layout
    else {
        return Err(SummaryError::Invalid("not a series"));
    };
    let mut w = SummaryWriter::new(buf, seq, head)?;
    let n = w.nodes();
    if nodes.len() != n || nodes.iter().any(|c| c.len() != usize::from(channels)) {
        return Err(SummaryError::Invalid(
            "nodes or channels differ from the head",
        ));
    }
    let body = w.body_mut();
    let mut at = 0;
    for stat in Stat::ALL.into_iter().filter(|s| stats.has(*s)) {
        let width = stat.width();
        for ch in 0..usize::from(channels) {
            for c in 0..usize::from(stat.components(element)) {
                for node in nodes {
                    let s = &node[ch];
                    let bytes = &mut body[at..at + width];
                    match stat {
                        Stat::Min => bytes.copy_from_slice(&s.min[c].to_bits().to_le_bytes()),
                        Stat::Max => bytes.copy_from_slice(&s.max[c].to_bits().to_le_bytes()),
                        Stat::Sum => bytes.copy_from_slice(&s.sum[c].to_le_bytes()),
                        Stat::SumSq => bytes.copy_from_slice(&s.sum_sq[c].to_le_bytes()),
                        Stat::Clipped => bytes.copy_from_slice(&s.clipped.to_le_bytes()),
                        Stat::Flags => bytes.copy_from_slice(&s.flags.to_le_bytes()),
                        Stat::PowerMean => bytes.copy_from_slice(&s.power_mean.to_le_bytes()),
                        Stat::PowerMax => bytes.copy_from_slice(&s.power_max.to_le_bytes()),
                    }
                    at += width;
                }
            }
        }
    }
    debug_assert_eq!(at, body.len(), "every body byte written");
    Ok(w.finish())
}

/// Nodes per block of a level: the most, a power of two, whose block of `node_len` bytes per node stays within
/// `max_block_bytes`; at least 1. Batches start on multiples of it, so a block's nodes are the children of whole
/// parents when it is 16 or more.
#[cfg(feature = "std")]
fn batch_nodes(node_len: usize, max_block_bytes: usize) -> u64 {
    let room = max_block_bytes.saturating_sub(HEADER_LEN + SUMMARY_FIXED_LEN + CRC_LEN);
    let fit = (room / node_len.max(1)).max(1) as u64;
    1u64 << (63 - fit.leading_zeros())
}

#[cfg(test)]
mod tests;

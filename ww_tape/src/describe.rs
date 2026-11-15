//! What describe records say: the producer, clocks, clock maps, tracks, API bundles and typed items.

use exact_units::units::{Per, Second, Tick};
use exact_units::{MagnitudeOf, Ratio64, Rounding};
use shrink_wrap::prelude::*;

#[cfg(feature = "std")]
use crate::FullVersionOwned;
use crate::{FNV_OFFSET, FullVersion, fnv1a};

/// What a describe block says, one per record on track 0.
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Clone), ww_repr = u4)]
pub enum Describe<'i> {
    Producer(Producer<'i>),
    Track(TrackDef<'i>),
    /// An API bundle (`ww_self` `ApiBundle` in its ShrinkWrap encoding) by its hash, the hash as the producer,
    /// [TrackDef::bundle] and [TypeRef::bundle] know it (wire_weaver's `ApiHash`). Written once per hash in a describe
    /// set; a live feed may leave it out and let the consumer look the hash up.
    Bundle {
        hash: RefVec<'i, u8>,
        bytes: RefVec<'i, u8>,
    },
    Clock(ClockDef<'i>),
    Map(ClockMap<'i>),
    /// Any wire_weaver value, typed by its bundle and type path: a run record, a media reference, settings.
    Item(TypedItem<'i>),
    /// How the file's blocks are placed; a file whose describe set has none is not aligned.
    Layout(Layout),
}

/// How blocks are placed in a file.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone, Copy),
    validate = Layout::validate
)]
pub struct Layout {
    /// 0, or [ALIGNMENT]: every chunk block's payload starts at a multiple of it in the file, its head and smaller
    /// blocks filling the gap before it. Series files are aligned, event files not.
    pub alignment: u32,
}

/// The alignment of an aligned file: 4 KiB, a page and a direct I/O block.
pub const ALIGNMENT: u32 = 4096;

impl Layout {
    /// Not aligned.
    pub const UNALIGNED: Layout = Layout { alignment: 0 };
    /// Chunk payloads on 4 KiB boundaries.
    pub const ALIGNED: Layout = Layout {
        alignment: ALIGNMENT,
    };

    fn validate(&self) -> Result<(), shrink_wrap::Error> {
        if self.alignment == 0 || self.alignment == ALIGNMENT {
            Ok(())
        } else {
            Err(shrink_wrap::Error::SubtypeOutOfRange)
        }
    }
}

/// Who writes the blocks.
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Clone))]
pub struct Producer<'i> {
    /// What it is, for people (`ww link tap`).
    pub name: &'i str,
}

/// A clock: what record times of the tracks on it count.
///
/// A device restart is a new clock (same counter, next epoch in its [key](Self::key)); how clocks relate is said only
/// by [ClockMap]s. Wall time is a clock too: origin [ClockOrigin::UnixEpoch], key [UNIX_CLOCK_KEY].
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Clone))]
pub struct ClockDef<'i> {
    /// Number in the describe set, 1 and up ([NO_CLOCK] is none): [TrackDef::clock] and [ClockMap] name it, and record
    /// blocks list it. Stays the same for the producer's life.
    pub number: UNib32,
    /// Stable identity across files and runs, see [clock_key].
    pub key: u64,
    /// Ticks per second, exact (`60000/1001`, `125 MHz`).
    pub rate: ClockRate,
    /// Seconds between two values the clock can really give, exact: `1/32768` for a 32 kHz counter read as
    /// nanoseconds. At least one tick.
    pub resolution: ClockResolution,
    pub origin: ClockOrigin,
    /// A name for people (`host`, `dev[3].boot[17]`).
    pub label: &'i str,
}

/// A clock's rate: ticks per second, exact. On the wire exactly its [Magnitude](exact_units::Magnitude).
pub type ClockRate = MagnitudeOf<Per<Tick, Second>>;

/// A clock's resolution: seconds between two values it can really give, exact. On the wire exactly its
/// [Magnitude](exact_units::Magnitude).
pub type ClockResolution = MagnitudeOf<Second>;

/// What tick 0 of a clock is, as an identity, never as a number.
#[derive_shrink_wrap(borrowed, owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone, Copy), ww_repr = u4)]
pub enum ClockOrigin {
    /// A counter with no known start.
    Free,
    /// A device's boot; the boot count is the epoch in the clock's key. A restart is a new clock of this origin with
    /// no exact relation to the one before: only an estimated [ClockMap] relates them.
    Boot,
    /// The Unix epoch, 1970-01-01T00:00:00 UTC.
    UnixEpoch,
    /// The start of a media file or stream (its time base).
    MediaStart,
    /// The start of the producer's run.
    RunStart,
    /// The next epoch of the same counter: a long capture at fine ticks is cut into clock epochs so ticks stay in
    /// i64. Its epoch link fixes tick 0: a [ClockMap] from this clock to the previous epoch's clock with one segment
    /// from tick 0 to the end, slope 1, error 0 and `b0 > 0`, so tick 0 here is tick `b0` there
    /// ([ClockMap::is_epoch_link]); a [DescribeSet](crate::DescribeSet) requires it. Unlike a restart
    /// ([Self::Boot]), the two clocks are one counter.
    Epoch,
}

/// A map from clock [from](Self::from) to clock [to](Self::to): segments in ascending, non-overlapping order.
///
/// A map is strictly increasing over all its segments, or item order would flip between the clocks: each segment
/// starts above the last value the one before maps to. A tick in no segment is unmapped. A forward jump (NTP step,
/// drift correction) starts a new segment; a backward step is a gap or a new clock, never a decreasing segment. A
/// map written here is known when the describe set is written (a wall clock reading at the start); a map estimated
/// over time is a track whose items are [MapSegment]s.
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Clone), validate = map_increasing)]
pub struct ClockMap<'i> {
    pub from: UNib32,
    pub to: UNib32,
    pub segments: RefVec<'i, MapSegment>,
}

/// `b = b0 + (a - a0) · slope` for `a` in `[a0, a1)` (`a1` None: to the end), within `± error` ticks of `to`.
///
/// The slope is a positive reduced fraction of `to` ticks per `from` tick ([Ratio64]), so a map is strictly
/// increasing. It is not a [Magnitude](exact_units::Magnitude), since a fitted ratio such as `1000000007/999999931` has primes a `Magnitude`
/// can't hold.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    derive(Debug, PartialEq, Eq, Clone, Copy),
    validate = MapSegment::validate
)]
pub struct MapSegment {
    pub a0: i64,
    pub a1: Option<i64>,
    pub b0: i64,
    pub slope: Ratio64,
    /// Bound of the error, in ticks of `to`.
    pub error: u64,
}

impl MapSegment {
    /// A segment; None for an empty range.
    pub fn new(a0: i64, a1: Option<i64>, b0: i64, slope: Ratio64, error: u64) -> Option<Self> {
        if a1.is_some_and(|a1| a1 <= a0) {
            return None;
        }
        Some(MapSegment {
            a0,
            a1,
            b0,
            slope,
            error,
        })
    }

    /// An offset: `b = b0 + (a - a0)`, slope 1, exact.
    pub const fn offset(a0: i64, b0: i64) -> Self {
        MapSegment {
            a0,
            a1: None,
            b0,
            slope: Ratio64::ONE,
            error: 0,
        }
    }

    /// The last value the segment maps to, at `a1 - 1`, rounded down; None for a segment to the end or past i64.
    pub fn last(&self) -> Option<i64> {
        self.map(self.a1?.checked_sub(1)?, Rounding::Floor)
    }

    /// Whether `b = b0 + (a - a0)` exactly: slope 1, error 0.
    pub fn is_exact_offset(&self) -> bool {
        self.slope == Ratio64::ONE && self.error == 0
    }

    /// `a` mapped, rounded by `rounding`; None outside the segment or when `a - a0` or the result leaves i64.
    pub fn map(&self, a: i64, rounding: Rounding) -> Option<i64> {
        if a < self.a0 || self.a1.is_some_and(|a1| a >= a1) {
            return None;
        }
        let d = a.checked_sub(self.a0)?;
        self.b0.checked_add(self.slope.apply_rounded(d, rounding)?)
    }

    fn validate(&self) -> Result<(), shrink_wrap::Error> {
        match self.a1 {
            Some(a1) if a1 <= self.a0 => Err(shrink_wrap::Error::SubtypeOutOfRange),
            _ => Ok(()),
        }
    }
}

/// Whether segments in this order make a strictly increasing map: each starts at or after the end of the one before
/// (which has an end) and above the last value it maps to.
pub fn segments_increasing(segments: impl IntoIterator<Item = MapSegment>) -> bool {
    increasing(segments.into_iter().map(Some))
}

/// [segments_increasing] over segments read one by one, None for one that didn't read.
fn increasing(segments: impl Iterator<Item = Option<MapSegment>>) -> bool {
    let mut prev: Option<MapSegment> = None;
    for s in segments {
        let Some(s) = s else { return false };
        if let Some(p) = prev {
            let (Some(a1), Some(last)) = (p.a1, p.last()) else {
                return false;
            };
            if s.a0 < a1 || s.b0 <= last {
                return false;
            }
        }
        prev = Some(s);
    }
    true
}

/// The segments of a [ClockMap], borrowed or owned, for its check on decode.
trait MapSegments {
    fn segment_list(&self) -> impl Iterator<Item = Option<MapSegment>>;
}

impl MapSegments for ClockMap<'_> {
    fn segment_list(&self) -> impl Iterator<Item = Option<MapSegment>> {
        self.segments.iter().map(Result::ok)
    }
}

#[cfg(feature = "std")]
impl MapSegments for ClockMapOwned {
    fn segment_list(&self) -> impl Iterator<Item = Option<MapSegment>> {
        self.segments.iter().copied().map(Some)
    }
}

impl ClockMap<'_> {
    /// Whether the map is strictly increasing over all its segments ([segments_increasing]).
    pub fn is_increasing(&self) -> bool {
        increasing(self.segment_list())
    }

    /// Whether the map is an epoch link ([ClockOrigin::Epoch]): from the new epoch's clock to the previous one's,
    /// one segment from tick 0 to the end, slope 1, error 0, `b0 > 0`.
    pub fn is_epoch_link(&self) -> bool {
        let mut segments = self.segment_list();
        match (segments.next(), segments.next()) {
            (Some(Some(s)), None) => {
                s.is_exact_offset()
                    && s.a0 == 0
                    && s.a1.is_none()
                    && s.b0 > 0
                    && self.from != self.to
            }
            _ => false,
        }
    }
}

fn map_increasing(m: &impl MapSegments) -> Result<(), shrink_wrap::Error> {
    if increasing(m.segment_list()) {
        Ok(())
    } else {
        Err(shrink_wrap::Error::SubtypeOutOfRange)
    }
}

/// A type in an API bundle: the bundle's hash, the crate the type is defined in and its path in that crate
/// (`units::Volt`, `RunRecord`), as `ww_self` names a definition.
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Clone))]
pub struct TypeRef<'i> {
    /// Hash of the bundle holding the definition, as in [Describe::Bundle].
    pub bundle: RefVec<'i, u8>,
    pub krate: FullVersion<'i>,
    /// Modules from the crate root and the type's name, `::` between them.
    pub path: &'i str,
}

/// A wire_weaver value of a type in a bundle.
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Clone))]
pub struct TypedItem<'i> {
    pub ty: TypeRef<'i>,
    /// The value in its ShrinkWrap encoding.
    pub value: RefVec<'i, u8>,
}

/// One track.
///
/// Its identity is [key](Self::key) plus [dimensions](Self::dimensions), see [track_id].
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Clone))]
pub struct TrackDef<'i> {
    /// Number in records, 1 and up (0 is [DESCRIBE_TRACK](crate::DESCRIBE_TRACK)); stays the same for the producer's
    /// life.
    pub number: UNib32,
    /// Stable key: the same track across files and producer runs, see [track_key].
    pub key: u64,
    /// The [ClockDef::number] record times of this track count.
    pub clock: UNib32,
    /// Name and value pairs that are part of the identity (`input=cam1`, `producer=vad`), in ascending order of
    /// name, names unique. Two variants of one track coexist when their dimensions differ.
    pub dimensions: RefVec<'i, Dimension<'i>>,
    /// The API the track's resource is in.
    pub api: FullVersion<'i>,
    /// Hash of that API's bundle, as in [Describe::Bundle]; empty when not known.
    pub bundle: RefVec<'i, u8>,
    /// Resource path of the track's stream in the API: `ww_trait` ids, array indices included.
    pub path: RefVec<'i, UNib32>,
    /// The item type when the path doesn't give it (a producer's result, a map track); None: the stream's type.
    pub item: Option<TypeRef<'i>>,
    /// A name for people (`link[1].rx.frames`); readers go by `path`.
    pub label: &'i str,
    /// The newest record of this track is repeated in every describe set.
    pub sticky: bool,
}

/// A name and value of a track's identity.
#[derive_shrink_wrap(owned(feature = "std"), derive(Debug, PartialEq, Eq, Clone))]
pub struct Dimension<'i> {
    pub name: &'i str,
    pub value: &'i str,
}

/// No clock: never a [ClockDef::number]; record blocks and tracks name clocks 1 and up.
pub const NO_CLOCK: u32 = 0;

/// Key of the wall clock (origin Unix epoch), the same for every producer.
pub const UNIX_CLOCK_KEY: u64 = fnv1a(FNV_OFFSET, b"ww_tape unix epoch");

/// Stable key of a clock: FNV-1a of `api_root` (u64 LE), every path id (u32 LE) of the thing that counts, then
/// `epoch` (u64 LE, a boot count).
pub fn clock_key(api_root: u64, path: &[u32], epoch: u64) -> u64 {
    fnv1a(crate::track_key(api_root, path), &epoch.to_le_bytes())
}

/// Id of a track's identity: its key without dimensions, else FNV-1a of the key (u64 LE) and of every dimension's
/// name and value, each followed by a 0 byte.
pub fn track_id<'a>(key: u64, dimensions: impl IntoIterator<Item = Dimension<'a>>) -> u64 {
    let mut h = None;
    for d in dimensions {
        let mut x = h.unwrap_or_else(|| fnv1a(FNV_OFFSET, &key.to_le_bytes()));
        x = fnv1a(fnv1a(x, d.name.as_bytes()), &[0]);
        x = fnv1a(fnv1a(x, d.value.as_bytes()), &[0]);
        h = Some(x);
    }
    h.unwrap_or(key)
}

impl TrackDef<'_> {
    /// Id of the track's identity, see [track_id].
    pub fn id(&self) -> u64 {
        track_id(self.key, self.dimensions.iter().filter_map(Result::ok))
    }

    /// Whether the dimensions are in ascending order of name, names unique.
    pub fn dimensions_canonical(&self) -> bool {
        let mut prev: Option<&str> = None;
        for d in self.dimensions.iter() {
            let Ok(d) = d else { return false };
            if prev.is_some_and(|p| p >= d.name) {
                return false;
            }
            prev = Some(d.name);
        }
        true
    }
}

/// Where a describe record goes in the canonical order: its rank, then what it is sorted by within the rank.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rank {
    Producer,
    Layout,
    Bundle,
    Clock(u32),
    Map {
        from: u32,
        to: u32,
    },
    Track(u32),
    /// Typed items and variants a reader doesn't know: in the order of their encoded bytes.
    Other,
}

impl Describe<'_> {
    /// Its place in the canonical order: producer, layout, bundles, clocks, maps, tracks by number, then typed items. Within
    /// a rank bundles go by hash and typed items by their encoded bytes ([DescribeSet](crate::DescribeSet)).
    pub fn rank(&self) -> Rank {
        match self {
            Describe::Producer(_) => Rank::Producer,
            Describe::Layout(_) => Rank::Layout,
            Describe::Bundle { .. } => Rank::Bundle,
            Describe::Clock(c) => Rank::Clock(c.number.0),
            Describe::Map(m) => Rank::Map {
                from: m.from.0,
                to: m.to.0,
            },
            Describe::Track(t) => Rank::Track(t.number.0),
            Describe::Item(_) | Describe::Unknown(_) => Rank::Other,
        }
    }
}

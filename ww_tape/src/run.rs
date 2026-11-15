//! Runs: one writer's unbroken append, named by a [RunTag] in every block header.

/// Names a run: one writer's unbroken append (a producer start, a device boot, an importer's pass over one source).
/// Every block of the run carries it at header bytes 24..28, and seq counts from 0 in every run.
///
/// A tag a writer makes is never 0 nor `u32::MAX`, so a blank header (all 0x00 or all 0xFF) never names a run.
/// [RunTag::NONE] (0) stands for no run: a block made before it is placed (a summary tree's, renumbered by
/// [TapeWriter::defer_block](crate::TapeWriter::defer_block)) and a piece head that names no previous run.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RunTag(u32);

impl RunTag {
    /// No run.
    pub const NONE: RunTag = RunTag(0);

    /// The tag as written in a header, any value (a reader's view: 0 and `u32::MAX` name no run a writer made).
    pub const fn from_u32(v: u32) -> Self {
        RunTag(v)
    }

    /// `v` as a tag a writer may use: 0 becomes 1 and `u32::MAX` becomes `u32::MAX - 1`, as logring does for its
    /// tag.
    pub const fn writable(v: u32) -> Self {
        match v {
            0 => RunTag(1),
            u32::MAX => RunTag(u32::MAX - 1),
            v => RunTag(v),
        }
    }

    pub const fn get(self) -> u32 {
        self.0
    }

    /// Whether a writer could have made it: neither 0 nor `u32::MAX`.
    pub const fn is_run(self) -> bool {
        self.0 != 0 && self.0 != u32::MAX
    }

    /// A tag from `seed` (a device's unique id, a free-running timer at boot, an ADC noise read; or anything that
    /// names the run, for a deterministic writer): the first 4 bytes of BLAKE3 in key derivation mode
    /// ("ww_tape run tag") over it, made [writable](Self::writable). The same seed gives the same tag.
    pub fn from_seed(seed: &[u8]) -> Self {
        let mut h = blake3::Hasher::new_derive_key("ww_tape run tag");
        h.update(seed);
        let mut out = [0u8; 4];
        h.finalize_xof().fill(&mut out);
        Self::writable(u32::from_le_bytes(out))
    }

    /// The tag of the run that starts a piece chain at the piece with `id`: [Self::from_seed] over the id, so a
    /// deterministic writer gives the same bytes for the same input. Piece ids are derived from the result id
    /// ([crate::PieceId::first]).
    pub fn of_piece(id: &crate::PieceId) -> Self {
        Self::from_seed(&id.0)
    }

    /// A random tag, from the OS random source (the keys of std's `RandomState`), the time and the process id.
    #[cfg(feature = "std")]
    pub fn random() -> Self {
        use std::hash::{BuildHasher, Hasher};
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        h.write_u128(now);
        h.write_u32(std::process::id());
        Self::from_seed(&h.finish().to_le_bytes())
    }
}

/// Which block of which run: the identity a block header carries (run tag at 24..28, seq at 16..24).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId {
    pub run: RunTag,
    pub seq: u64,
}

impl BlockId {
    pub const fn new(run: RunTag, seq: u64) -> Self {
        BlockId { run, seq }
    }

    /// The id of the block after this one in the same run.
    #[must_use]
    pub const fn next(self) -> Self {
        BlockId {
            run: self.run,
            seq: self.seq.wrapping_add(1),
        }
    }
}

/// What a tape's bytes are kept on: says where a reader takes a new run.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum Medium {
    /// A file: any describe block (another producer run appended) or piece head (concatenated pieces) may start a
    /// new run. A reader finds pieces from the end of the file.
    #[default]
    File,
    /// A raw medium (SD card, NAND, a partition) that may hold stale blocks of earlier lives behind the end of data:
    /// a new run starts only at a piece head naming the run just ended ([crate::PieceHead::prev_run]), and a reader
    /// walks forward from the start or a hint, never from the medium's end.
    Raw,
}

use ww_api::prelude::*;
use ww_si::units::{Block, Chunk};
use ww_si::{Bytes, Count, Index};

use crate::Error;

/// Most chunk states one `Inventory::chunks` reply carries. A node may return fewer (see there). Raising it is
/// compatible.
pub const MAX_CHUNKS_PAGE: usize = 64;

/// Length of a chunk hash: SHA-256 truncated to its first 8 bytes. Fixed by the protocol spec; a different length
/// is a new major version of this API.
pub const CHUNK_HASH_LEN: usize = 8;

/// What the node holds now, chunk by chunk: the manifest the host plans against.
///
/// The host usually doesn't need it: when [NodeInfo::manifest_digest] matches the manifest of a release it built,
/// it already knows every chunk. The full read is for the cases where it doesn't (a development build, an image
/// changed by an interrupted update, a device the host has never seen).
#[ww_trait]
pub trait Inventory {
    /// Chunk size, chunk count and erase layout of a slot, as this node sees them.
    ///
    /// [Error::Unsupported] on [Slot::Inactive] when this node has no previous image to report on.
    #[errors(Unsupported)]
    fn layout(slot: Slot) -> Result<SlotLayout, Error>;

    /// Digest of a slot's manifest, as the protocol spec defines it. Equal digests mean equal manifests, so a host
    /// that already has a manifest with this digest needn't read it.
    ///
    /// Hashing a whole slot takes time (about a second per few hundred KB on a small MCU); the node may answer later
    /// and report progress on `Stage`'s `event` stream, if it has one.
    ///
    /// [Error::Unsupported] on [Slot::Inactive] when this node has no previous image to report on.
    #[errors(Unsupported)]
    fn manifest_digest(slot: Slot) -> Result<[u8; 8], Error>;

    /// The state of `count` chunks of `slot`, starting at chunk `first`.
    ///
    /// Paged on purpose: the host sets the pace and asks again for a page whose reply was lost. Fewer entries than
    /// asked for come back at the end of the slot, past [MAX_CHUNKS_PAGE], or when a page would not fit the node's
    /// reply buffer; the host continues from the first chunk it didn't get.
    ///
    /// [Error::Unsupported] on [Slot::Inactive] when this node has no previous image to report on,
    /// [Error::OutOfRange] when `first` is past the end of the slot.
    #[errors(Unsupported, OutOfRange)]
    fn chunks(
        slot: Slot,
        first: Index<Chunk, u16>,
        count: Count<Chunk, u16>,
    ) -> Result<MaxLen<RefVec<'i, ChunkState>, MAX_CHUNKS_PAGE>, Error>;
}

/// A slot of the flash, from the update's point of view.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = nib
)]
pub enum Slot {
    /// The image that runs (or, in an updater, the app slot being rewritten).
    Active,
    /// The slot a live A/B update is written to: DFU, the other bank, the other partition.
    Inactive,
}

/// Layout of a slot, as `Inventory::layout` reports it.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct SlotLayout {
    /// Bytes per chunk: the unit hashed in the manifest and written by one WRITE step.
    pub chunk_size: Bytes<u32>,
    /// Chunks in the slot.
    pub chunks: Count<Chunk, u16>,
    /// Bytes per erase block (the unit of an ERASE step). On parts with mixed sector sizes, the largest.
    pub erase_size: Bytes<u32>,
    /// Erase blocks in the slot.
    pub erase_blocks: Count<Block, u16>,
}

/// The state of one chunk, as the manifest records it.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = u2
)]
pub enum ChunkState {
    /// Readable data with this hash.
    Hash([u8; 8]),
    /// Erased: every byte reads as erased. May be written without an erase.
    Blank,
    /// Faults on read (ECC: a write or erase cut by power loss). Needs an erase before anything else.
    Unreadable,
}

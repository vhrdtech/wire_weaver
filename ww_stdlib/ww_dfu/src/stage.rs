use wire_weaver::prelude::*;
use ww_si::exact_units::niche::{Niched, Null};
use ww_si::prefix::One;
use ww_si::units::{Block, Byte, Chunk, Request, Step};
use ww_si::{Bytes, Count, Fixed, Index, Range};

use crate::{BootState, Error};

/// Largest data payload of one `Stage::write` call or one [PlanSegment], in bytes, for every node.
///
/// A node takes at most its own [StageLimits::max_piece], which may be smaller. Raising this bound later is a
/// compatible change (old nodes and hosts keep working, the wire form doesn't change); lowering it is breaking.
pub const MAX_PIECE_LEN: usize = 1024;

/// Largest plan header in bytes, as `Stage::begin` and `StageBulk::begin_bulk` take it. The header's real length is
/// fixed by the protocol spec and is far smaller; this only bounds the message. Raising it is compatible.
pub const MAX_HEADER_LEN: usize = 256;

/// Receive a plan step by step, apply it ([Target::Live]) or store it ([Target::Stored]), and finish it.
///
/// One session at a time per node. The step methods must arrive in plan order: a step with a higher index than
/// expected is [Error::OutOfOrder] with the index the node expects, so the host can go back; a step with a lower
/// index is accepted again and skipped (its reply was lost), as long as it is the same step.
#[ww_trait]
#[claim(exclusive)]
pub trait Stage {
    /// What this node accepts: piece size, how many calls may be in flight, targets, the plan area.
    fn limits() -> StageLimits;

    /// Start a session for the plan with this header (the header's octets as the protocol spec defines them), or
    /// resume the unfinished session for the same header.
    ///
    /// Everything that can be checked before the first byte is written is checked here: the header's format, its
    /// geometry against this node's, the image's security counter against the floor, the descriptor signature, the boot
    /// state ([BootState::may_stage]), the target against [StageLimits], and for [Target::Stored] that the plan fits
    /// the plan area. A refusal here costs the host nothing but this call, which matters on a slow link.
    ///
    /// [Error::BadHeader]/[Error::BadSignature]: the header or its signature doesn't parse or verify;
    /// [Error::Unsupported]: `target` isn't one of [StageLimits::live]/[StageLimits::stored];
    /// [Error::WrongGeometry]: another flash layout; [Error::BelowFloor]: security counter below the floor;
    /// [Error::PlanTooBig]: [Target::Stored] doesn't fit the plan area; [Error::WrongState]: the boot state
    /// doesn't allow staging ([BootState::may_stage]).
    #[errors(
        BadHeader,
        BadSignature,
        Unsupported,
        WrongGeometry,
        BelowFloor,
        PlanTooBig,
        WrongState
    )]
    fn begin(
        target: Target,
        header: MaxLen<RefVec<'i, u8>, MAX_HEADER_LEN>,
    ) -> Result<Session, Error>;

    /// Step `index`: erase erase-block `block` of the slot being written.
    ///
    /// [Error::NoSession]: no session in progress; [Error::OutOfRange]: `index` is past [Session::steps];
    /// [Error::OutOfOrder]: `index` is ahead of what the node expects; [Error::StepMismatch]: a resend under an
    /// index the node already has, but a different step; [Error::PlanTooBig]: [Target::Stored] overflows the plan
    /// area; [Error::Flash]/[Error::Ecc]: the flash itself faulted.
    #[errors(
        NoSession,
        OutOfRange,
        OutOfOrder,
        StepMismatch,
        PlanTooBig,
        Flash,
        Ecc
    )]
    fn erase(index: Index<Step, u32>, block: Index<Block, u16>) -> Result<(), Error>;

    /// Step `index`: copy chunk `src` of the running image into chunk `dst` of the slot being written.
    ///
    /// The node hashes `src` first; when it doesn't match `hash`, nothing is written and the answer is
    /// [Error::SourceMismatch] (the host planned against a manifest that is no longer true; it should read the
    /// manifest again or send the chunk as a WRITE). In place, the planner never copies from a chunk an earlier step
    /// has overwritten. See `erase` for the other step errors.
    #[errors(
        NoSession,
        OutOfRange,
        OutOfOrder,
        StepMismatch,
        SourceMismatch,
        PlanTooBig,
        Flash,
        Ecc
    )]
    fn copy(
        index: Index<Step, u32>,
        dst: Index<Chunk, u16>,
        src: Index<Chunk, u16>,
        hash: [u8; 8],
    ) -> Result<(), Error>;

    /// Step `index`, or a piece of it: program chunk `dst` with data whose whole chunk hashes to `hash`.
    ///
    /// A chunk bigger than [StageLimits::max_piece] comes in pieces, in order, each saying where it goes in the chunk
    /// (`offset`, bytes from the chunk's start). The node hashes the chunk as the pieces arrive and checks `hash` when
    /// the last byte of the chunk is in; a mismatch is [Error::HashMismatch] and the host sends the whole chunk again
    /// from offset 0. Every piece repeats `dst` and `hash`, so a piece is never applied to the wrong step.
    /// [Error::PieceTooBig]: `data` is longer than [StageLimits::max_piece]. See `erase` for the other step errors.
    #[errors(
        NoSession,
        OutOfRange,
        OutOfOrder,
        StepMismatch,
        PieceTooBig,
        HashMismatch,
        PlanTooBig,
        Flash,
        Ecc
    )]
    fn write(
        index: Index<Step, u32>,
        dst: Index<Chunk, u16>,
        hash: [u8; 8],
        offset: Index<Byte, u32>,
        data: MaxLen<RefVec<'i, u8>, MAX_PIECE_LEN>,
    ) -> Result<(), Error>;

    /// Where the current session stands: the next step the node expects, and how far into it.
    ///
    /// [Error::NoSession] when nothing is in progress.
    #[errors(NoSession)]
    fn progress() -> Result<Progress, Error>;

    /// All steps are in: verify and stage.
    ///
    /// - [Target::Live] in the inactive slot (A/B): the image is checked against the descriptor (hash and
    ///   signature) and marked staged; it boots for its trial after `BootControl::reboot` with [BootTarget::App].
    /// - [Target::Stored]: the stored plan is checked for completeness and its header signature; it is applied by
    ///   the bootloader at the next reset.
    /// - [Target::Live] in place (an updater): the new image is checked against the descriptor and marked bootable.
    ///
    /// Verification hashes the whole image and may take a few seconds; the node answers when it is done and reports
    /// progress on `event` meanwhile. The answer is the boot state the node is now in.
    ///
    /// [Error::NoSession]: nothing in progress; [Error::Incomplete]: not every step is in yet;
    /// [Error::VerifyFailed]: the finished image doesn't match its descriptor; [Error::BadHeader]: the stored
    /// header doesn't parse ([Target::Stored]); [Error::OutOfRange]/[Error::SourceMismatch]/[Error::HashMismatch]:
    /// the stored plan's own re-check fails ([Target::Stored]).
    #[errors(
        NoSession,
        Incomplete,
        VerifyFailed,
        BadHeader,
        OutOfRange,
        SourceMismatch,
        HashMismatch
    )]
    fn finish() -> Result<BootState, Error>;

    /// Drop the current session. What was written stays where it was (an inactive slot or a plan area is just
    /// scratch); in place, the device stays in the updater until a session finishes.
    fn abort();

    /// Progress of long operations (hashing, copying, verifying, applying a stored plan) and the outcome of a session,
    /// for the host's progress bar and log. Nothing here is needed for correctness: the method replies are.
    stream!(event: StageEvent);
}

/// Receive a whole plan in one go, for links where a round trip per step would cost more than the delta saves.
///
/// Uses the session control of `Stage` on the same node (`Stage::progress`, `Stage::finish`, `Stage::abort`); only
/// the start and the data path differ. The plan after its header is a byte stream: the steps with each WRITE's data
/// right after its step, exactly the protocol spec's wire form, so with [Target::Stored] the node writes these bytes
/// to its plan area unchanged.
///
/// Flow: `begin_bulk` → segments on `plan` (a credit sink) until `total_len` bytes are in, each placed by its offset
/// → the node acknowledges on `plan_ack` at the end → the host calls `Stage::finish`.
#[ww_trait]
#[claim(exclusive)]
pub trait StageBulk {
    /// Start (or resume) a bulk session for the plan with this header; the checks are those of `Stage::begin`.
    ///
    /// `total_len` is the length of the plan after the header. `ack_every` asks the node to acknowledge at least
    /// every so many bytes even when nothing went wrong; none (`AckEvery::from(None)`, `0` on the wire) means only at
    /// the end (and on a failure), the right choice on a reliable link (one acknowledgement per update). The answer's
    /// [Session::resume_byte] says from which byte to start sending (0 for a new session).
    ///
    /// Same checks as `Stage::begin`, plus [Error::BadHeader] when `total_len` doesn't match the header.
    #[errors(
        BadHeader,
        BadSignature,
        Unsupported,
        WrongGeometry,
        BelowFloor,
        PlanTooBig,
        WrongState
    )]
    fn begin_bulk(
        target: Target,
        header: MaxLen<RefVec<'i, u8>, MAX_HEADER_LEN>,
        total_len: Bytes<u32>,
        ack_every: AckEvery,
    ) -> Result<Session, Error>;

    /// Plan bytes from the host. A segment may be any size up to [StageLimits::max_piece]; each says where its bytes
    /// go.
    ///
    /// A credit sink: credit counts the bytes of segments as sent (data plus the offset and encoding around it), so
    /// a buffer of any size holds as many segments as fit, small ones don't take the room of a large one. The node
    /// grants as it frees bytes; its buffer must hold at least the largest segment, `max_piece` of data plus up to
    /// 7 bytes (the generated `PLAN_MAX_ITEM_BYTES` gives it for [MAX_PIECE_LEN], `SinkCredit::sized_for` checks it).
    /// A lost segment is detected and sent again by the sink's flow control (go-back-N), segments arrive in order. A segment whose offset does not
    /// continue what the node has (e.g., the host resumed from the wrong place) answers [BulkStatus::Gap] on
    /// `plan_ack`.
    #[credit]
    sink!(plan: PlanSegment<'i>);

    /// Acknowledgements from the node: how far it is and whether something went wrong. How much more it can take is
    /// the `plan` sink's credit.
    ///
    /// Sent once at the end (all bytes in, or an error), on an offset gap, and every `ack_every` bytes if asked.
    stream!(plan_ack: PlanAck);
}

/// Where a plan goes on the node.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = nib
)]
pub enum Target {
    /// Apply each step as it arrives: into the inactive slot (A/B, mode 1) or, in an updater, in place (mode 3).
    Live,
    /// Store the plan with its data in the plan area; the bootloader applies it in place after the reset (mode 2).
    Stored,
}

/// What `Stage::limits` reports.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct StageLimits {
    /// Largest data payload in one `Stage::write` or [PlanSegment], 1 to [MAX_PIECE_LEN] bytes. Chosen to fit the
    /// node's receive buffer.
    pub max_piece: Bytes<u16, Range<1, { MAX_PIECE_LEN as i64 }>>,
    /// How many step calls the host may have outstanding at once, at least 1. `1` on a node that blocks while it
    /// writes flash.
    pub in_flight: Fixed<Request, One, u8, Range<1, 255>>,
    /// [Target::Live] is accepted.
    pub live: bool,
    /// [Target::Stored] is accepted.
    pub stored: bool,
    /// Size of the plan area (the largest plan [Target::Stored] takes), `None` without one.
    pub plan_area: PlanArea,
    /// The node keeps working (answers other requests) while it writes flash. False on single-bank parts that stall
    /// while programming their own flash.
    pub writes_in_background: bool,
}

/// What `Stage::begin` and `StageBulk::begin_bulk` answer.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct Session {
    /// Steps in the plan, from its header.
    pub steps: Count<Step, u32>,
    /// `false` for a new session, `true` when an unfinished one for the same plan was found.
    pub resumed: bool,
    /// Where `Stage` continues: the next step (`0` for a new session).
    pub resume_step: Index<Step, u32>,
    /// Where `StageBulk` continues: the next byte after the header (`0` for a new session).
    pub resume_byte: Index<Byte, u32>,
}

/// What `Stage::progress` answers.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct Progress {
    /// Steps in the plan.
    pub steps: Count<Step, u32>,
    /// The next step the node expects (`steps` when all are in).
    pub next_step: Index<Step, u32>,
    /// For a WRITE in pieces: where the next piece of the next step's chunk starts, the bytes already in.
    pub chunk_offset: Index<Byte, u32>,
    /// For a bulk session: the next byte after the header the node expects, all before it received in order.
    pub plan_offset: Index<Byte, u32>,
}

// TODO(P2401#11nov-1946): drop the spelled-out `u32` once the walker reads Niched's default storage parameter
/// `StageBulk::begin_bulk`'s `ack_every`: acknowledge at least every so many bytes, or `None` (`0` on the wire) for
/// only at the end.
pub type AckEvery = Niched<Fixed<Byte, One, u32, Range<1, 4294967295>>, Null<0>, u32>;

/// [StageLimits::plan_area]: the plan area's size, or `None` (`0` on the wire) without one.
pub type PlanArea = Niched<Fixed<Byte, One, u32, Range<1, 4294967295>>, Null<0>, u32>;

/// A segment of a bulk plan, sent on `StageBulk`'s `plan` sink.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug)
)]
pub struct PlanSegment<'i> {
    /// Where these bytes go: offset from the first byte after the plan header.
    pub offset: Index<Byte, u32>,
    /// Plan bytes, at most [StageLimits::max_piece] (and never more than [MAX_PIECE_LEN]).
    pub data: MaxLen<RefVec<'i, u8>, MAX_PIECE_LEN>,
}

/// An acknowledgement on `StageBulk`'s `plan_ack` stream.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct PlanAck {
    /// The first byte after the header the node doesn't have; it has every byte before it. After a
    /// [BulkStatus::Gap] the host resends from here.
    pub have: Index<Byte, u32>,
    /// How the session stands.
    pub status: BulkStatus,
}

/// Status carried by a [PlanAck].
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = nib
)]
pub enum BulkStatus {
    /// Receiving; nothing wrong.
    Receiving,
    /// A segment did not continue at `have` (e.g., a wrong resume point): bytes from `have` on are missing; resend
    /// from there. Segments lost on the way are handled by the `plan` sink's flow control instead.
    Gap,
    /// All `total_len` bytes are in; call `Stage::finish`.
    Complete,
    /// The session failed; nothing more is accepted.
    Failed(Error),
}

/// Progress of long operations and session outcomes, on `Stage`'s `event` stream.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = nib
)]
pub enum StageEvent {
    /// Hashing a slot for the manifest: `done` of `total` chunks.
    Hashing {
        done: Count<Chunk, u16>,
        total: Count<Chunk, u16>,
    },
    /// Applying: `done` of `total` steps.
    Applying {
        done: Count<Step, u32>,
        total: Count<Step, u32>,
    },
    /// Verifying the finished image or stored plan: `done` of `total` bytes.
    Verifying { done: Bytes<u32>, total: Bytes<u32> },
    /// The session finished; the node is now in this state.
    Finished(BootState),
    /// The session failed.
    Failed(Error),
}

//! Firmware update API: a set of small traits that every node taking part in an update picks from.
//!
//! The update itself is a **plan**: a header (which image it produces, for which flash geometry) and a list of steps
//! that turn what the device holds into the new image: ERASE a block, COPY a chunk the device already has, WRITE a
//! chunk whose data comes with the step. The plan format, the manifest and the signed image descriptor are defined in
//! octets by the chunky protocol spec (`SPEC.md` in `chunky_mcu_updater`); this crate only carries them. Signed and
//! stored records (the descriptor, the plan header) travel as **raw octets**, byte for byte as the spec lays them
//! out, so a C implementation with its own framing and a Rust one over WireWeaver move identical bytes, and the
//! shared test vectors hold for both. Session-level values (states, errors, limits, progress) are WireWeaver types.
//!
//! A plain full-image update is the same thing: a plan made only of ERASE and WRITE steps. A node that does not do
//! deltas implements the same traits and simply never sees a COPY.
//!
//! # Traits
//!
//! | Trait           | What it is for                                                         |
//! |-----------------|------------------------------------------------------------------------|
//! | `UpdateNode`    | Who this node is and what it offers. Every node implements it.         |
//! | `BootControl`   | Boot state, confirm, revert, reboot into the app or the updater.       |
//! | `Inventory`     | What the node holds now: the manifest, chunk by chunk.                 |
//! | `Stage`         | Receive a plan step by step and apply or store it; session control.    |
//! | `StageBulk`     | Receive a whole plan as one stream (slow and remote links).            |
//!
//! Nothing forces a node to implement all of them. A host reads `UpdateNode::node` first and finds out from
//! [Offers] what this node can do. If it can't receive an update itself, the host calls `BootControl::reboot` with
//! [BootTarget::Updater], reconnects to the same device (by serial number) and carries on with whatever answers
//! there.
//!
//! # Three ways a device updates
//!
//! **1. A/B, applied live** ([Profile::Swap], [Profile::BankSwap], [Profile::RpBootrom]). The running app receives
//! the plan and applies it straight into the inactive slot (DFU, the other bank, the other partition). Nothing it
//! runs from is touched, so a cut or a lost link costs nothing but time. When the image is complete and verified,
//! `Stage::finish` marks it staged; `BootControl::reboot` with [BootTarget::App] switches to it for one trial boot,
//! and the new image must `BootControl::confirm` or the next reset reverts it.
//! Nodes: the app implements everything; the bootloader has no transport at all.
//!
//! **2. In place, stored plan** ([Profile::InPlace] with [Target::Stored]). The running app receives the whole plan,
//! data included, into a plan area in flash, while it keeps doing its job (a download over LoRa may take hours).
//! `Stage::finish` checks the descriptor signature and that the plan is complete **before anything is touched**.
//! After the reboot the bootloader applies the stored plan in place, offline; after a power cut it simply starts the
//! plan again (every step's result is checked by its hash, and the WRITE data is still in the plan area). Downtime is
//! the apply time only, and the bootloader still has no transport. Costs: a plan area sized for the largest delta,
//! and no revert (the old image is gone once applied). A plan bigger than the area falls back to mode 3.
//! Nodes: the app implements `UpdateNode`, `BootControl`, `Inventory`, `Stage` (and `StageBulk` on slow links).
//!
//! **3. In place, applied live** ([Profile::InPlace] with [Target::Live]). The bootloader, or a small
//! write-protected recovery app at its own address, receives the plan and applies it as the pieces arrive. No extra
//! flash, the fastest end to end, but the device does nothing else for the whole download, and a cut leaves no
//! bootable app, so this receiver must come back up and finish the job.
//! Nodes: the app implements `UpdateNode` and `BootControl` (often only `reboot(Updater)`); the updater implements
//! `UpdateNode`, `BootControl`, `Inventory`, `Stage` and maybe `StageBulk`.
//!
//! # Two ways a plan travels
//!
//! **Step by step** (`Stage`): one method call per step, each answered with a named error or success. Every call
//! carries its step index, and the device checks each step's result by hash, so resending a call whose reply was lost
//! is harmless. The host keeps up to [StageLimits::in_flight] calls outstanding, which hides most of the round trip
//! on fast links (USB, Ethernet, RTT).
//!
//! **In one go** (`StageBulk`): on a link where a round trip takes seconds (LoRa, LTE-M, satellite), per-step calls
//! would eat the whole gain of sending a delta. The host announces the plan once, then streams it, header excluded,
//! as raw octets; the device applies (modes 1 and 3) or stores (mode 2) as the bytes arrive and acknowledges only
//! when it has to: on a gap, when the host is about to run out of credit, and at the end. On a reliable link that is
//! a single acknowledgement for the whole update.
//!
//! # Resuming
//!
//! A session is identified by its plan header. `Stage::begin` (or `StageBulk::begin_bulk`) with the header of an
//! unfinished session does not start over: it answers with the progress already made, and the host continues from
//! there. The progress record is the flash itself (a step whose result is already in place is skipped by its hash),
//! so a session survives a lost link, a host restart and, for modes 2 and 3, a power cut.
//!
//! # Trust
//!
//! The device trusts nothing it receives. The host, the link and any relay in between (a gateway that stores and
//! forwards a plan, a broker) may be broken or hostile. So the device itself checks:
//! - every index and offset against its own layout before it touches flash (a step that points outside the slot is
//!   [Error::OutOfRange], never a write);
//! - every COPY source and every WRITE against the hash in its step;
//! - the plan header against its geometry, its version floor and the release signature, before the first step, and
//!   the finished image against the descriptor, before it is staged.
//!
//! Per-chunk hashes are not a trust boundary on their own; the signed descriptor over the whole image is.
//!
//! # Sizes
//!
//! Byte payloads (`Stage::write` data, [PlanSegment] data) are bounded by [StageLimits::max_piece], which the
//! device chooses to fit its receive buffer. Until WireWeaver has bounded sizes in the type system (FEATURES SW-19),
//! that limit is only checked at runtime: a longer piece is [Error::PieceTooBig].
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

/// Length of a chunk hash: SHA-256 truncated to its first 8 bytes. Fixed by the protocol spec; a different length
/// is a new major version of this API.
pub const CHUNK_HASH_LEN: usize = 8;

/// Who this node is and what it can do. Every node that takes part in updates implements it: the app, the
/// bootloader if it has a transport, a recovery app.
///
/// This is what a host reads first, and on slow links often the only thing it reads before an update: together with
/// [NodeInfo::manifest_digest] it tells the host which plan to send.
#[ww_trait]
pub trait UpdateNode {
    /// Identity, versions and state of this node, and which of the other update traits it implements.
    fn node() -> NodeInfo<'i>;
}

/// Boot state and the switch between images.
///
/// Implemented by the app (the minimum is `BootControl::reboot` with [BootTarget::Updater], for a node that
/// receives updates elsewhere) and by an updater (bootloader with a transport, recovery app), where it at least
/// reboots back into the app.
#[ww_trait]
pub trait BootControl {
    /// Where the device stands between two updates.
    fn boot_state() -> BootState;

    /// The running image works: keep it.
    ///
    /// Only meaningful in [BootState::Trial]; in any other state it does nothing and returns `Ok`. An image that
    /// never confirms is reverted at the next reset, so the app should confirm only after it has checked what
    /// matters to it (its peripherals, its link to the host, its config).
    fn confirm() -> Result<(), Error>;

    /// Give up the running trial image now, or un-stage a staged one: the previous image runs after the next reset.
    ///
    /// [Error::Unsupported] on profiles without a previous image ([Profile::InPlace]), [Error::WrongState] outside
    /// [BootState::Trial] and [BootState::Staged].
    fn revert() -> Result<(), Error>;

    /// Reset into the app or into the updater.
    ///
    /// The reply is sent before the reset, so the host knows the request arrived; the node resets shortly after
    /// (implementations wait long enough for the reply to leave, typically a few tens of milliseconds). The host then
    /// reconnects to the same device by serial number; what answers may be a different firmware with a different API
    /// (an updater after [BootTarget::Updater]).
    ///
    /// [BootTarget::App] after a successful `Stage::finish` is what starts the update: the bootloader switches to the
    /// staged image (A/B) or applies the stored plan (in place).
    fn reboot(into: BootTarget) -> Result<(), Error>;
}

/// What the node holds now, chunk by chunk: the manifest the host plans against.
///
/// The host usually doesn't need it: when [NodeInfo::manifest_digest] matches the manifest of a release it built,
/// it already knows every chunk. The full read is for the cases where it doesn't (a development build, an image
/// changed by an interrupted update, a device the host has never seen).
#[ww_trait]
pub trait Inventory {
    /// Chunk size, chunk count and erase layout of a slot, as this node sees them.
    fn layout(slot: Slot) -> Result<SlotLayout, Error>;

    /// Digest of a slot's manifest, as the protocol spec defines it. Equal digests mean equal manifests, so a host
    /// that already has a manifest with this digest needn't read it.
    ///
    /// Hashing a whole slot takes time (about a second per few hundred KB on a small MCU); the node may answer later
    /// and report progress on `Stage`'s `event` stream, if it has one.
    fn manifest_digest(slot: Slot) -> Result<[u8; 8], Error>;

    /// The state of `count` chunks of `slot`, starting at chunk `first`.
    ///
    /// Paged on purpose: the host sets the pace and asks again for a page whose reply was lost. Fewer entries than
    /// asked for come back at the end of the slot, or when a page would not fit the node's reply buffer; the host
    /// continues from the first chunk it didn't get.
    fn chunks(slot: Slot, first: u16, count: u16) -> Result<RefVec<'i, ChunkState>, Error>;
}

/// Receive a plan step by step, apply it ([Target::Live]) or store it ([Target::Stored]), and finish it.
///
/// One session at a time per node. The step methods must arrive in plan order: a step with a higher index than
/// expected is [Error::OutOfOrder] with the index the node expects, so the host can go back; a step with a lower
/// index is accepted again and skipped (its reply was lost), as long as it is the same step.
#[ww_trait]
pub trait Stage {
    /// What this node accepts: piece size, how many calls may be in flight, targets, the plan area.
    fn limits() -> StageLimits;

    /// Start a session for the plan with this header (the header's octets as the protocol spec defines them), or
    /// resume the unfinished session for the same header.
    ///
    /// Everything that can be checked before the first byte is written is checked here: the header's format, its
    /// geometry against this node's, the image version against the version floor, the descriptor signature, the boot
    /// state ([BootState::may_stage]), the target against [StageLimits], and for [Target::Stored] that the plan fits
    /// the plan area. A refusal here costs the host nothing but this call, which matters on a slow link.
    fn begin(target: Target, header: RefVec<'i, u8>) -> Result<Session, Error>;

    /// Step `index`: erase erase-block `block` of the slot being written.
    fn erase(index: u32, block: u16) -> Result<(), Error>;

    /// Step `index`: copy chunk `src` of the running image into chunk `dst` of the slot being written.
    ///
    /// The node hashes `src` first; when it doesn't match `hash`, nothing is written and the answer is
    /// [Error::SourceMismatch] (the host planned against a manifest that is no longer true; it should read the
    /// manifest again or send the chunk as a WRITE). In place, the planner never copies from a chunk an earlier step
    /// has overwritten.
    fn copy(index: u32, dst: u16, src: u16, hash: [u8; 8]) -> Result<(), Error>;

    /// Step `index`, or a piece of it: program chunk `dst` with data whose whole chunk hashes to `hash`.
    ///
    /// A chunk bigger than [StageLimits::max_piece] comes in pieces, in order, each saying where it goes in the chunk
    /// (`offset`, bytes from the chunk's start). The node hashes the chunk as the pieces arrive and checks `hash` when
    /// the last byte of the chunk is in; a mismatch is [Error::HashMismatch] and the host sends the whole chunk again
    /// from offset 0. Every piece repeats `dst` and `hash`, so a piece is never applied to the wrong step.
    fn write(
        index: u32,
        dst: u16,
        hash: [u8; 8],
        offset: u32,
        data: RefVec<'i, u8>,
    ) -> Result<(), Error>;

    /// Where the current session stands: the next step the node expects, and how far into it.
    ///
    /// [Error::NoSession] when nothing is in progress.
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
/// Flow: `begin_bulk` → segments on `plan` until `total_len` bytes are in, each placed by its offset → the node
/// acknowledges on `plan_ack` when it has to → the host calls `Stage::finish`.
#[ww_trait]
pub trait StageBulk {
    /// Start (or resume) a bulk session for the plan with this header; the checks are those of `Stage::begin`.
    ///
    /// `total_len` is the length of the plan after the header. `ack_every` asks the node to acknowledge at least
    /// every so many bytes even when nothing went wrong; `0` means only on gaps, credit and at the end, the right
    /// choice on a reliable link (one acknowledgement per update). The answer's [Session::resume_from] says from which
    /// byte to start sending (0 for a new session).
    fn begin_bulk(
        target: Target,
        header: RefVec<'i, u8>,
        total_len: u32,
        ack_every: u32,
    ) -> Result<Session, Error>;

    /// Plan bytes from the host. A segment may be any size up to [StageLimits::max_piece]; each says where its bytes
    /// go, so a node that sees a gap knows it and asks for the missing part.
    ///
    /// The node takes segments in order only: one that starts past the end of what the node has is a gap, so the
    /// node drops it and acknowledges with the offset it needs (go-back-N); one it already has is dropped silently.
    sink!(plan: PlanSegment<'i>);

    /// Acknowledgements from the node: how far it is, how much more it can take, and whether something went wrong.
    ///
    /// Sent when the host is about to run out of credit, on a gap, every `ack_every` bytes if asked, and once at the
    /// end (all bytes in, or an error). The host must not send past `have + credit`.
    ///
    /// Once WireWeaver's credit-based sinks land, the credit moves into the sink's own flow control and this stream
    /// keeps `have` and `status` only.
    stream!(plan_ack: PlanAck);
}

/// What `UpdateNode::node` reports.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug)
)]
pub struct NodeInfo<'i> {
    /// Which firmware is answering.
    pub role: Role,
    /// How this device switches between images.
    pub profile: Profile,
    /// Product name, as the release's manifest names it; the host uses it to pick the release line.
    pub product: &'i str,
    /// Flash geometry id as the protocol spec defines it (chip line, slot layout, chunk size). A plan is made for one
    /// geometry and is refused on any other.
    pub geometry_id: u32,
    /// Version of the image in the app slot, `None` when there is no valid image (an updater after a cut in mode 3).
    pub app_version: Option<u32>,
    /// Lowest version this device will still accept (anti-rollback). Only ever goes up.
    pub version_floor: u32,
    /// Digest of the app slot's manifest, if this node has it at hand (cached, or computed at boot); `None` means
    /// "read it with `Inventory::manifest_digest`".
    pub manifest_digest: Option<[u8; 8]>,
    /// Where the device stands between two updates.
    pub boot_state: BootState,
    /// How the last update ended, for the host's log and a rollout's statistics.
    pub last_result: LastResult,
    /// Which update traits this node implements.
    pub offers: Offers,
}

/// Which firmware is answering.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = nib
)]
pub enum Role {
    /// The application.
    App,
    /// The bootloader, with a transport of its own (mode 3).
    Bootloader,
    /// A recovery app at its own address that updates the app in place (mode 3).
    Recovery,
}

/// How a device switches between images; decides which [Target]s make sense.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = nib
)]
pub enum Profile {
    /// Two slots swapped page by page by the bootloader (embassy-boot), trial boot and revert.
    Swap,
    /// Dual-bank flash, the hardware swaps the banks (STM32 H7, G4, L4...), trial boot and revert.
    BankSwap,
    /// RP2350 boot ROM A/B partitions with try-before-you-buy.
    RpBootrom,
    /// One app slot, updated in place: from a stored plan by the bootloader (mode 2) or live by an updater (mode 3).
    /// No revert.
    InPlace,
}

/// Which update traits a node implements. Lets a host decide what to do after a single call, which matters on slow
/// links where introspection is expensive.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct Offers {
    /// `BootControl` is here.
    pub boot_control: bool,
    /// `Inventory` is here.
    pub inventory: bool,
    /// `Stage` is here.
    pub stage: bool,
    /// `StageBulk` is here.
    pub stage_bulk: bool,
}

/// Where the device stands between two updates.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = nib
)]
pub enum BootState {
    /// The running image is confirmed. An update may be started.
    Confirmed,
    /// The last update did not confirm and was reverted. An update may be started.
    Reverted,
    /// An image is staged (A/B) and switches in at the next reset.
    Staged,
    /// The running image is on its trial boot: it must `BootControl::confirm`, or the next reset reverts it.
    Trial,
    /// A complete, verified plan is in the plan area and is applied in place at the next reset (mode 2).
    PlanStored,
    /// In place, an update is under way and the app slot does not hold a bootable image (mode 3, in the updater).
    Updating,
}

impl BootState {
    /// True when a new update may be started: not while the inactive slot holds the image to switch to or to revert
    /// to, and not while a stored plan waits to be applied.
    pub fn may_stage(self) -> bool {
        matches!(
            self,
            BootState::Confirmed | BootState::Reverted | BootState::Updating
        )
    }
}

/// Where `BootControl::reboot` goes.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = nib
)]
pub enum BootTarget {
    /// The app: the normal reset. Switches to a staged image or applies a stored plan if there is one.
    App,
    /// The updater (bootloader with a transport, or recovery app), to receive an update there (mode 3).
    Updater,
}

/// How the last update ended.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct LastResult {
    /// What happened.
    pub outcome: Outcome,
    /// The version the update was for, `0` when there was none.
    pub version: u32,
}

/// What happened to the last update.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = nib
)]
pub enum Outcome {
    /// No update since the device was provisioned.
    Nothing,
    /// Updated and confirmed.
    Confirmed,
    /// The trial image did not confirm (or was reverted on purpose) and the previous one runs again.
    Reverted,
    /// Refused or abandoned before staging (bad signature, below the floor, a failed step, aborted).
    Rejected,
    /// A stored plan or a live in-place update could not finish and the updater took over.
    Recovery,
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
    pub chunk_size: u32,
    /// Chunks in the slot.
    pub chunk_count: u16,
    /// Bytes per erase block (the unit of an ERASE step). On parts with mixed sector sizes, the largest.
    pub erase_size: u32,
    /// Erase blocks in the slot.
    pub erase_count: u16,
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
    /// Largest data payload in one `Stage::write` or [PlanSegment], in bytes. Chosen to fit the node's receive
    /// buffer.
    pub max_piece: u32,
    /// How many step calls the host may have outstanding at once. `1` on a node that blocks while it writes flash.
    pub in_flight: u8,
    /// [Target::Live] is accepted.
    pub live: bool,
    /// [Target::Stored] is accepted.
    pub stored: bool,
    /// Size of the plan area in bytes (the largest plan [Target::Stored] takes), `0` without one.
    pub plan_area: u32,
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
    pub steps: u32,
    /// `false` for a new session, `true` when an unfinished one for the same plan was found.
    pub resumed: bool,
    /// Where to continue: the next step for `Stage`, the next byte after the header for `StageBulk` (`0` for a new
    /// session).
    pub resume_from: u32,
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
    pub steps: u32,
    /// The next step the node expects (`steps` when all are in).
    pub next_step: u32,
    /// For a WRITE in pieces: bytes of the next step's chunk already in.
    pub chunk_offset: u32,
    /// For a bulk session: bytes after the header received in order.
    pub plan_bytes: u32,
}

/// A segment of a bulk plan, sent on `StageBulk`'s `plan` sink.
#[derive_shrink_wrap(
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug)
)]
pub struct PlanSegment<'i> {
    /// Where these bytes go: offset from the first byte after the plan header.
    pub offset: u32,
    /// Plan bytes, at most [StageLimits::max_piece].
    pub data: RefVec<'i, u8>,
}

/// An acknowledgement on `StageBulk`'s `plan_ack` stream.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq)
)]
pub struct PlanAck {
    /// Bytes after the header the node has, in order. After a gap the host resends from here.
    pub have: u32,
    /// Bytes past `have` the host may send before the next acknowledgement.
    pub credit: u32,
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
    /// A gap: bytes from `have` on are missing; resend from there.
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
    Hashing { done: u16, total: u16 },
    /// Applying: `done` of `total` steps.
    Applying { done: u32, total: u32 },
    /// Verifying the finished image or stored plan: `done` of `total` bytes.
    Verifying { done: u32, total: u32 },
    /// The session finished; the node is now in this state.
    Finished(BootState),
    /// The session failed.
    Failed(Error),
}

/// Why a request was refused.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr_borrowed(feature = "defmt", derive(defmt::Format)),
    derive(Copy, Clone, Debug, PartialEq, Eq),
    self_describing,
    ww_repr = unib32
)]
pub enum Error {
    /// This node doesn't do that (a target it doesn't accept, revert without a previous image).
    Unsupported,
    /// Not in this boot state; the state is given.
    WrongState(BootState),
    /// Another session is in progress; `Stage::abort` it first.
    Busy,
    /// No session in progress.
    NoSession,
    /// The plan header is malformed (magic, format, reserved bytes).
    BadHeader,
    /// The plan is for another flash geometry.
    WrongGeometry,
    /// The image's version is below the version floor.
    BelowFloor,
    /// The release signature over the descriptor doesn't verify.
    BadSignature,
    /// A step, offset or chunk points outside the slot. Nothing was written.
    OutOfRange,
    /// A step arrived ahead of its turn; the node expects this step index next.
    OutOfOrder(u32),
    /// A different step arrived under an index the node already has.
    StepMismatch,
    /// A piece is longer than [StageLimits::max_piece].
    PieceTooBig,
    /// The plan doesn't fit the plan area ([Target::Stored]).
    PlanTooBig,
    /// A COPY source doesn't have the hash its step says. Nothing was written.
    SourceMismatch,
    /// A written chunk doesn't have the hash its step says.
    HashMismatch,
    /// `Stage::finish` before all steps are in.
    Incomplete,
    /// The finished image doesn't match its descriptor.
    VerifyFailed,
    /// The flash reported an error.
    Flash,
    /// A read faulted on ECC (a cut write or erase); the chunk needs an erase.
    Ecc,
    /// An implementation-specific error.
    Custom(u32),
}

//! Cost of the server's call gate on the device (SRV-50): a server of `tests/dfu_api` (the five `ww_dfu` traits at
//! the root) built without the gate and with `call_gate = true` (feature `call-gate`), whose gate admits
//! `ww_dfu::BootControl` and `ww_dfu::UpdateNode` while guarded and refuses the rest, as a guarded board does. Each
//! request is bracketed by `mark_start` / `mark_end` for `qemu_icount`, the first bracket is empty (calibration):
//!
//! 1. `BootControl::boot_state`, not guarded: the gate admits at once
//! 2. `BootControl::boot_state`, guarded: admitted by its trait (two string compares)
//! 3. `Inventory::layout`, not guarded: the handler answers `Unimplemented`
//! 4. `Inventory::layout`, guarded: refused, `ErrorKind::Refused` with a 31-byte reason
//!
//! ```text
//! cargo build --profile fw-z --bin qemu-call-gate [--features call-gate]
//! cd ../qemu_icount && cargo run --release -- cortex-m3 lm3s6965evb ../mcu_qemu/target/thumbv7m-none-eabi/fw-z/qemu-call-gate
//! ```
//!
//! Measured 3 Nov 2026 (Cortex-M3 under QEMU, instructions, not cycles), at `fw-z` (`opt-level = "z"`, fat LTO, the
//! profile firmware ships with) and `fw-3`:
//!
//! | request                          | off, z | gate, z | off, 3 | gate, 3 |
//! |----------------------------------|-------:|--------:|-------:|--------:|
//! | 1. `boot_state`, not guarded     |   1802 |    1835 |   1035 |    1073 |
//! | 2. `boot_state`, guarded         |   1802 |    1863 |   1035 |    1088 |
//! | 3. `layout`, not guarded         |   2135 |    2176 |   1275 |    1306 |
//! | 4. `layout`, guarded (refused)   |   2135 |    2552 |   1275 |    1624 |
//!
//! The gate costs 33-41 instructions per request at `z` when it admits (61 when it compares a trait: a pointer
//! compare per name, the linker merges the firmware's `TraitId` strings with the generated ones); a refusal
//! replaces the handler with writing the reason. Flash: 35150 -> 35922 B text at `z` (+772 B), 55018 -> 56734 B at
//! `3` (+1716 B), for the 21 items of the five traits: a `Target` (16 B) and its path per item, one shared
//! `ww_gate_refused`, the op and observe helpers. Without `call_gate` nothing is generated: `qemu-global-paths` was
//! the same against the commit before SRV-50 but 4 B of text (the new `ErrorKind::Refused` arm of `Error`'s
//! serializer) and 2 instructions on its `TraitNotFound` answer.
#![no_main]
#![no_std]

extern crate panic_semihosting;

use core::fmt::Write;
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hio};
use wire_weaver::prelude::*;
use wire_weaver::shrink_wrap::tail_bytes::TailBytes;
use ww_client_server::{PathKind, Request, RequestKind};

#[inline(never)]
fn mark_start() {
    unsafe { core::arch::asm!("nop", options(nostack, preserves_flags)) }
}

#[inline(never)]
fn mark_end() {
    unsafe { core::arch::asm!("nop", "nop", options(nostack, preserves_flags)) }
}

/// A `Call` request with seq 1, as it goes on the wire (seq in one byte).
fn request<'b>(path: &[UNib32], args: &[u8], buf: &'b mut [u8]) -> &'b [u8] {
    let request = Request {
        seq: UVlq32Backfill(1),
        priority: ww_client_server::Priority::NORMAL.nibble(),
        path_kind: PathKind::absolute(path),
        kind: RequestKind::Call {
            args: TailBytes(args),
        },
    };
    let len = request.to_ww_bytes(buf).unwrap().len();
    Request::set_seq(&mut buf[..len], 1).unwrap()
}

#[entry]
fn main() -> ! {
    let mut stdout = hio::hstdout().unwrap();
    // `boot` is item 1 of the root, `boot_state` item 0 of it; `inventory` is item 2, `layout` item 0
    let boot_state = [UNib32(1), UNib32(0)];
    let layout = [UNib32(2), UNib32(0)];
    let (mut b0, mut b1) = ([0u8; 64], [0u8; 64]);
    let mut slot = [0u8; 4];
    let slot = ww_dfu::Slot::Active.to_ww_bytes(&mut slot).unwrap();
    let boot_state = request(&boot_state, &[], &mut b0);
    let layout = request(&layout, slot, &mut b1);
    let requests = [(boot_state, false), (boot_state, true), (layout, false), (layout, true)];

    let mut scratch = [0u8; 128];
    let mut event_scratch = [0u8; 128];
    let mut server = ServerState {
        bulk_plan_credit: ww_stream::sink::SinkCredit::new(256),
        guarded: false,
    };
    let mut out = EventWriter::new(DummyTx, &mut event_scratch);
    mark_start();
    mark_end();
    for (request, guarded) in requests {
        server.guarded = core::hint::black_box(guarded);
        let request = core::hint::black_box(request);
        mark_start();
        let r = server.process_request_bytes(request, &mut scratch, &mut out, ());
        mark_end();
        writeln!(stdout, "guarded {guarded}: {} B request -> {:02x?}", request.len(), r).unwrap();
    }

    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}

#[cfg(feature = "call-gate")]
mod gate {
    use wire_weaver::gate::{CallGate, Op, Target, TraitId};

    const BOOT_CONTROL: TraitId = TraitId::new("ww_dfu", "BootControl");
    const UPDATE_NODE: TraitId = TraitId::new("ww_dfu", "UpdateNode");

    /// A guarded board's gate: boot control and the node's identity only.
    impl CallGate for super::ServerState {
        fn admit(&self, target: &Target, _op: Op) -> Result<(), &str> {
            if !self.guarded || target.is_in(&BOOT_CONTROL) || target.is_in(&UPDATE_NODE) {
                Ok(())
            } else {
                Err("board guarded: unknown rework 9")
            }
        }
    }
}

struct DummyTx;
impl wire_weaver::BlockingMessageSink for DummyTx {
    fn send(&mut self, _message: &[u8]) -> Result<(), ()> {
        Ok(())
    }
}

pub struct ServerState {
    /// Credit sink state of `bulk_plan`: how much the device can buffer, `free()` it after use
    pub bulk_plan_credit: ww_stream::sink::SinkCredit,
    /// The board is guarded: the gate refuses all but boot control and the node's identity
    #[cfg_attr(not(feature = "call-gate"), allow(dead_code))]
    pub guarded: bool,
}

impl ServerState {
    // ---- dfu_api::DfuDevice at root ----

    // ---- ww_dfu::UpdateNode at node ----

    /// Identity, versions and state of this node, and which of the other update traits it implements.
    fn node_node(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> RpcResult<ww_dfu::NodeInfo<'_>> {
        Unimplemented.into()
    }

    // ---- ww_dfu::BootControl at boot ----

    /// Where the device stands between two updates.
    fn boot_boot_state(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> RpcResult<ww_dfu::BootState> {
        RpcResult::Ready(ww_dfu::BootState::Confirmed)
    }

    /// The running image works: keep it.
    ///
    /// Only meaningful in [BootState::Trial]; in any other state it does nothing and returns `Ok`. An image that
    /// never confirms is reverted at the next reset, so the app should confirm only after it has checked what
    /// matters to it (its peripherals, its link to the host, its config).
    fn boot_confirm(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> RpcResult<Result<(), ww_dfu::Error>> {
        Unimplemented.into()
    }

    /// Give up the running trial image now, or un-stage a staged one: the previous image runs after the next reset.
    ///
    /// [Error::Unsupported] on profiles without a previous image ([Profile::InPlace]), [Error::WrongState] outside
    /// [BootState::Trial] and [BootState::Staged].
    fn boot_revert(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> RpcResult<Result<(), ww_dfu::Error>> {
        Unimplemented.into()
    }

    /// Reset into the app or into the updater.
    ///
    /// The reply is sent before the reset, so the host knows the request arrived; the node resets shortly after
    /// (implementations wait long enough for the reply to leave, typically a few tens of milliseconds). The host then
    /// reconnects to the same device by serial number; what answers may be a different firmware with a different API
    /// (an updater after [BootTarget::Updater]).
    ///
    /// [BootTarget::App] after a successful `Stage::finish` is what starts the update: the bootloader switches to the
    /// staged image (A/B) or applies the stored plan (in place).
    ///
    /// [Error::Unsupported] into [BootTarget::Updater] on an A/B node (there is no separate updater to reboot into).
    fn boot_reboot(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _into: ww_dfu::BootTarget,
    ) -> RpcResult<Result<(), ww_dfu::Error>> {
        Unimplemented.into()
    }

    // ---- ww_dfu::Inventory at inventory ----

    /// Chunk size, chunk count and erase layout of a slot, as this node sees them.
    ///
    /// [Error::Unsupported] on [Slot::Inactive] when this node has no previous image to report on.
    fn inventory_layout(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _slot: ww_dfu::Slot,
    ) -> RpcResult<Result<ww_dfu::SlotLayout, ww_dfu::Error>> {
        Unimplemented.into()
    }

    /// Digest of a slot's manifest, as the protocol spec defines it. Equal digests mean equal manifests, so a host
    /// that already has a manifest with this digest needn't read it.
    ///
    /// Hashing a whole slot takes time (about a second per few hundred KB on a small MCU); the node may answer later
    /// and report progress on `Stage`'s `event` stream, if it has one.
    ///
    /// [Error::Unsupported] on [Slot::Inactive] when this node has no previous image to report on.
    fn inventory_manifest_digest(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _slot: ww_dfu::Slot,
    ) -> RpcResult<Result<[u8; 8], ww_dfu::Error>> {
        Unimplemented.into()
    }

    /// The state of `count` chunks of `slot`, starting at chunk `first`.
    ///
    /// Paged on purpose: the host sets the pace and asks again for a page whose reply was lost. Fewer entries than
    /// asked for come back at the end of the slot, past [MAX_CHUNKS_PAGE], or when a page would not fit the node's
    /// reply buffer; the host continues from the first chunk it didn't get.
    ///
    /// [Error::Unsupported] on [Slot::Inactive] when this node has no previous image to report on,
    /// [Error::OutOfRange] when `first` is past the end of the slot.
    fn inventory_chunks(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _slot: ww_dfu::Slot,
        _first: u16,
        _count: u16,
    ) -> RpcResult<
        Result<
            wire_weaver::shrink_wrap::MaxLen<
                wire_weaver::shrink_wrap::RefVec<'_, ww_dfu::ChunkState>,
                64,
            >,
            ww_dfu::Error,
        >,
    > {
        Unimplemented.into()
    }

    // ---- ww_dfu::Stage at stage ----

    /// What this node accepts: piece size, how many calls may be in flight, targets, the plan area.
    fn stage_limits(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> RpcResult<ww_dfu::StageLimits> {
        Unimplemented.into()
    }

    /// Start a session for the plan with this header (the header's octets as the protocol spec defines them), or
    /// resume the unfinished session for the same header.
    ///
    /// Everything that can be checked before the first byte is written is checked here: the header's format, its
    /// geometry against this node's, the image version against the version floor, the descriptor signature, the boot
    /// state ([BootState::may_stage]), the target against [StageLimits], and for [Target::Stored] that the plan fits
    /// the plan area. A refusal here costs the host nothing but this call, which matters on a slow link.
    ///
    /// [Error::BadHeader]/[Error::BadSignature]: the header or its signature doesn't parse or verify;
    /// [Error::Unsupported]: `target` isn't one of [StageLimits::live]/[StageLimits::stored];
    /// [Error::WrongGeometry]: another flash layout; [Error::BelowFloor]: image version below the floor;
    /// [Error::PlanTooBig]: [Target::Stored] doesn't fit the plan area; [Error::WrongState]: the boot state
    /// doesn't allow staging ([BootState::may_stage]).
    fn stage_begin(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _target: ww_dfu::Target,
        _header: wire_weaver::shrink_wrap::MaxLen<wire_weaver::shrink_wrap::RefVec<'_, u8>, 256>,
    ) -> RpcResult<Result<ww_dfu::Session, ww_dfu::Error>> {
        Unimplemented.into()
    }

    /// Step `index`: erase erase-block `block` of the slot being written.
    ///
    /// [Error::NoSession]: no session in progress; [Error::OutOfRange]: `index` is past [Session::steps];
    /// [Error::OutOfOrder]: `index` is ahead of what the node expects; [Error::StepMismatch]: a resend under an
    /// index the node already has, but a different step; [Error::PlanTooBig]: [Target::Stored] overflows the plan
    /// area; [Error::Flash]/[Error::Ecc]: the flash itself faulted.
    fn stage_erase(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _index: u32,
        _block: u16,
    ) -> RpcResult<Result<(), ww_dfu::Error>> {
        Unimplemented.into()
    }

    /// Step `index`: copy chunk `src` of the running image into chunk `dst` of the slot being written.
    ///
    /// The node hashes `src` first; when it doesn't match `hash`, nothing is written and the answer is
    /// [Error::SourceMismatch] (the host planned against a manifest that is no longer true; it should read the
    /// manifest again or send the chunk as a WRITE). In place, the planner never copies from a chunk an earlier step
    /// has overwritten. See `erase` for the other step errors.
    fn stage_copy(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _index: u32,
        _dst: u16,
        _src: u16,
        _hash: [u8; 8],
    ) -> RpcResult<Result<(), ww_dfu::Error>> {
        Unimplemented.into()
    }

    /// Step `index`, or a piece of it: program chunk `dst` with data whose whole chunk hashes to `hash`.
    ///
    /// A chunk bigger than [StageLimits::max_piece] comes in pieces, in order, each saying where it goes in the chunk
    /// (`offset`, bytes from the chunk's start). The node hashes the chunk as the pieces arrive and checks `hash` when
    /// the last byte of the chunk is in; a mismatch is [Error::HashMismatch] and the host sends the whole chunk again
    /// from offset 0. Every piece repeats `dst` and `hash`, so a piece is never applied to the wrong step.
    /// [Error::PieceTooBig]: `data` is longer than [StageLimits::max_piece]. See `erase` for the other step errors.
    fn stage_write(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _index: u32,
        _dst: u16,
        _hash: [u8; 8],
        _offset: u32,
        _data: wire_weaver::shrink_wrap::MaxLen<wire_weaver::shrink_wrap::RefVec<'_, u8>, 1024>,
    ) -> RpcResult<Result<(), ww_dfu::Error>> {
        Unimplemented.into()
    }

    /// Where the current session stands: the next step the node expects, and how far into it.
    ///
    /// [Error::NoSession] when nothing is in progress.
    fn stage_progress(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> RpcResult<Result<ww_dfu::Progress, ww_dfu::Error>> {
        Unimplemented.into()
    }

    /// All steps are in: verify and stage.
    ///
    /// - [Target::Live] in the inactive slot (A/B): the image is checked against the descriptor (hash and
    ///  signature) and marked staged; it boots for its trial after `BootControl::reboot` with [BootTarget::App].
    /// - [Target::Stored]: the stored plan is checked for completeness and its header signature; it is applied by
    ///  the bootloader at the next reset.
    /// - [Target::Live] in place (an updater): the new image is checked against the descriptor and marked bootable.
    ///
    /// Verification hashes the whole image and may take a few seconds; the node answers when it is done and reports
    /// progress on `event` meanwhile. The answer is the boot state the node is now in.
    ///
    /// [Error::NoSession]: nothing in progress; [Error::Incomplete]: not every step is in yet;
    /// [Error::VerifyFailed]: the finished image doesn't match its descriptor; [Error::BadHeader]: the stored
    /// header doesn't parse ([Target::Stored]); [Error::OutOfRange]/[Error::SourceMismatch]/[Error::HashMismatch]:
    /// the stored plan's own re-check fails ([Target::Stored]).
    fn stage_finish(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
    ) -> RpcResult<Result<ww_dfu::BootState, ww_dfu::Error>> {
        Unimplemented.into()
    }

    /// Drop the current session. What was written stays where it was (an inactive slot or a plan area is just
    /// scratch); in place, the device stays in the updater until a session finishes.
    fn stage_abort(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
        Unimplemented.into()
    }

    /// Progress of long operations (hashing, copying, verifying, applying a stored plan) and the outcome of a session,
    /// for the host's progress bar and log. Nothing here is needed for correctness: the method replies are.
    ///
    /// Opens the stream (a new session): `Ok` / `Ready` starts it.
    fn stage_event(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
        RpcResult::Ready(())
    }

    /// The host closed the stream.
    fn close_stage_event(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

    fn sideband_stage_event(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _sideband: ww_client_server::StreamSideband,
    ) -> Option<ww_client_server::StreamSideband> {
        // TODO: None sends no reply
        None
    }

    // ---- ww_dfu::StageBulk at bulk ----

    /// Start (or resume) a bulk session for the plan with this header; the checks are those of `Stage::begin`.
    ///
    /// `total_len` is the length of the plan after the header. `ack_every` asks the node to acknowledge at least
    /// every so many bytes even when nothing went wrong; `0` means only at the end (and on a failure), the right
    /// choice on a reliable link (one acknowledgement per update). The answer's [Session::resume_from] says from which
    /// byte to start sending (0 for a new session).
    ///
    /// Same checks as `Stage::begin`, plus [Error::BadHeader] when `total_len` doesn't match the header.
    fn bulk_begin_bulk(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _target: ww_dfu::Target,
        _header: wire_weaver::shrink_wrap::MaxLen<wire_weaver::shrink_wrap::RefVec<'_, u8>, 256>,
        _total_len: u32,
        _ack_every: u32,
    ) -> RpcResult<Result<ww_dfu::Session, ww_dfu::Error>> {
        Unimplemented.into()
    }

    /// Credit state of `plan`: generated code opens and closes it, feeds it the host's writes and queries and sends its acknowledgements, see docs/api/credit_sinks.md.
    fn credit_bulk_plan(&mut self) -> &mut ww_stream::sink::SinkCredit {
        &mut self.bulk_plan_credit
    }

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
    fn sideband_bulk_plan(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _sideband: ww_client_server::StreamSideband,
    ) -> Option<ww_client_server::StreamSideband> {
        // TODO: None sends no reply
        None
    }

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
    fn write_bulk_plan(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _value: ww_dfu::PlanSegment<'_>,
    ) {
    }

    /// Acknowledgements from the node: how far it is and whether something went wrong. How much more it can take is
    /// the `plan` sink's credit.
    ///
    /// Sent once at the end (all bytes in, or an error), on an offset gap, and every `ack_every` bytes if asked.
    ///
    /// Opens the stream (a new session): `Ok` / `Ready` starts it.
    fn bulk_plan_ack(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) -> RpcResult<()> {
        RpcResult::Ready(())
    }

    /// The host closed the stream.
    fn close_bulk_plan_ack(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>) {}

    fn sideband_bulk_plan_ack(
        &mut self,
        _cx: &mut Context<'_, impl BlockingEventOut>,
        _sideband: ww_client_server::StreamSideband,
    ) -> Option<ww_client_server::StreamSideband> {
        // TODO: None sends no reply
        None
    }
}

#[cfg(not(feature = "call-gate"))]
pub mod api_impl {
    wire_weaver::ww_codegen!(
        dfu_api :: DfuDevice for super::ServerState,
        server = true, no_alloc = true, use_async = false,
        method_model = "_=immediate",
        property_model = "_=get_set",
    );
}

#[cfg(feature = "call-gate")]
pub mod api_impl {
    wire_weaver::ww_codegen!(
        dfu_api :: DfuDevice for super::ServerState,
        server = true, no_alloc = true, use_async = false,
        method_model = "_=immediate",
        property_model = "_=get_set",
        call_gate = true,
    );
}

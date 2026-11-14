//! Firmware update API: a set of small traits that every node taking part in an update picks from.
//!
//! **Start with the [guide](https://ww.vhrd.tech/std_library/ww_dfu/)** (`docs/std_library/ww_dfu.md` in the
//! wire_weaver repo): it names the eight ways a device updates (`2C-APP-STEP` Swap Shuttle to `1L-UPD-BULK` Pit-stop
//! Freight), helps pick one in three questions, and walks through each with its flash layout, flow, power-cut
//! behaviour and the exchange recorded from its end-to-end test. The rest of this page is the reference.
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
//! **1. A/B, applied live** ([Profile::AB]; scenarios `2C-*` and `2R-*`). The running app receives the plan
//! and applies it straight into the inactive slot (DFU, the other bank, the other partition). Nothing it
//! runs from is touched, so a cut or a lost link costs nothing but time. When the image is complete and verified,
//! `Stage::finish` marks it staged; `BootControl::reboot` with [BootTarget::App] switches to it for one trial boot,
//! and the new image must `BootControl::confirm` or the next reset reverts it.
//! Nodes: the app implements everything; the bootloader has no transport at all.
//!
//! **2. In place, stored plan** ([Profile::InPlace] with [Target::Stored]; scenarios `1S-*`). The running app
//! receives the whole plan, data included, into a plan area in flash, while it keeps doing its job (a download over
//! LoRa may take hours). `Stage::finish` checks the descriptor signature and that the plan is complete **before
//! anything is touched**. After the reboot the bootloader applies the stored plan in place, offline; after a power cut
//! it resumes from its apply journal (every step's result is checked by its hash, and the WRITE data is still in the
//! plan area). Downtime is the apply time only, and the bootloader still has no transport. Costs: a plan area sized
//! for the largest delta, and no revert (the old image is gone once applied). A plan bigger than the area falls back
//! to mode 3.
//! Nodes: the app implements `UpdateNode`, `BootControl`, `Inventory`, `Stage` (and `StageBulk` on slow links).
//!
//! **3. In place, applied live** ([Profile::InPlace] with [Target::Live]; scenarios `1L-*`). The bootloader, or a
//! small write-protected recovery app at its own address, receives the plan and applies it as the pieces arrive. No
//! extra flash, the fastest end to end, but the device does nothing else for the whole download, and a cut leaves no
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
//! as raw octets on a credit sink (WireWeaver's credit-based flow control, `docs/api/streams.md`): the host never
//! sends more bytes than the device has room for, lost segments are sent again from the last one the device has
//! (go-back-N). The device applies (modes 1 and 3) or stores (mode 2) as the bytes arrive and acknowledges on its own
//! only at the end (and every `ack_every` bytes if asked). On a reliable link that is a single acknowledgement for
//! the whole update, besides the sink's credit grants, which the device sends as often as its buffer needs.
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
//! - the plan header against its geometry, its security floor and the release signature, before the first step, and
//!   the finished image against the descriptor, before it is staged.
//!
//! Per-chunk hashes are not a trust boundary on their own; the signed descriptor over the whole image is.
//!
//! # Sizes
//!
//! Byte payloads (`Stage::write` data, [PlanSegment] data) have two limits:
//! - [MAX_PIECE_LEN], the same for every node, is part of the types (`MaxLen`). Both ends check it, and a device
//!   can check at compile time that its receive buffer holds the largest request (the generated
//!   `<METHOD>_REQUEST_MAX_LEN` constants).
//! - [StageLimits::max_piece], at most [MAX_PIECE_LEN], is what this node takes, chosen to fit its buffer. A longer
//!   piece is [Error::PieceTooBig].
#![cfg_attr(not(feature = "std"), no_std)]

pub mod boot_control;
pub use boot_control::{BootControl, BootState, BootTarget};

pub mod inventory;
pub use inventory::{CHUNK_HASH_LEN, ChunkState, Inventory, MAX_CHUNKS_PAGE, Slot, SlotLayout};

pub mod stage;
pub use stage::{
    AckEvery, BulkStatus, MAX_HEADER_LEN, MAX_PIECE_LEN, PlanAck, PlanArea, PlanSegment, Progress,
    Session, Stage, StageBulk, StageEvent, StageLimits, Target,
};

pub mod update_node;
pub use update_node::{
    GeometryId, ImageInfo, ImageVersion, LastResult, MAX_PRODUCT_LEN, NodeInfo, Offers, Outcome,
    ProductId, Profile, Role, Seal, SecurityCounter, SlotSwitch, UpdateNode,
};

pub mod error;
pub use error::Error;

#[cfg(test)]
mod tests {
    use super::*;
    use wire_weaver::prelude::*;
    use ww_si::units::{Chunk, Step};
    use ww_si::{Bytes, Count, Fixed, Index, Range};

    fn bytes<T: SerializeShrinkWrap>(value: &T) -> Vec<u8> {
        let mut buf = [0u8; 64];
        value.to_ww_bytes(&mut buf).unwrap().to_vec()
    }

    /// The ids, versions and typed numbers are their `T` on the wire, byte for byte.
    #[test]
    fn typed_numbers_are_their_storage() {
        let x = 0x1234_5678u32;
        assert_eq!(bytes(&GeometryId(x)), bytes(&x));
        assert_eq!(bytes(&ProductId(x)), bytes(&x));
        assert_eq!(bytes(&ImageVersion(x)), bytes(&x));
        assert_eq!(bytes(&SecurityCounter(x)), bytes(&x));
        assert_eq!(bytes(&Index::<Step, u32>::new(x)), bytes(&x));
        assert_eq!(bytes(&Bytes::<u32>::new(x)), bytes(&x));
        assert_eq!(bytes(&Count::<Chunk, u16>::new(7)), bytes(&7u16));
        let max_piece: Bytes<u16, Range<1, { MAX_PIECE_LEN as i64 }>> =
            Fixed::try_new(1024).unwrap();
        assert_eq!(bytes(&max_piece), bytes(&1024u16));
    }

    /// `None` of a zero-means-none field is a plain `0`, a value its number.
    #[test]
    fn niches_are_zero() {
        assert_eq!(bytes(&AckEvery::from(None)), bytes(&0u32));
        let every: AckEvery = Fixed::try_new(4096).ok().into();
        assert_eq!(bytes(&every), bytes(&4096u32));
        assert_eq!(every.value().map(|b| b.get()), Some(4096));
        assert_eq!(
            PlanArea::from_ww_bytes(&bytes(&0u32)).unwrap().value(),
            None
        );
    }

    /// A `max_piece` outside 1..=1024 is refused on decode.
    #[test]
    fn max_piece_bounds() {
        let limits = StageLimits {
            max_piece: Fixed::try_new(64).unwrap(),
            in_flight: Fixed::try_new(1).unwrap(),
            live: true,
            stored: false,
            plan_area: None.into(),
            writes_in_background: true,
        };
        let mut wire = bytes(&limits);
        assert_eq!(StageLimits::from_ww_bytes(&wire).unwrap(), limits);
        // max_piece is the first two bytes: 2000 is past the bound
        wire[..2].copy_from_slice(&2000u16.to_le_bytes());
        assert!(StageLimits::from_ww_bytes(&wire).is_err());
        wire[..2].copy_from_slice(&0u16.to_le_bytes());
        assert!(StageLimits::from_ww_bytes(&wire).is_err());
    }
}

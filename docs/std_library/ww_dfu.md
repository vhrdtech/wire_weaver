---
level: basic
---

# ww_dfu: firmware updates

`ww_dfu` is the firmware update API every WireWeaver device shares: a handful of small traits a node picks from, and
eight ways to put them together, depending on how much flash the device has, who receives the update and how fast
the link is. The docs are organized by those eight **scenarios**. Read the overview on this page, pick your scenario
with the questions below, then read only its section, on the [A/B](ww_dfu_ab.md) or the [one-slot](ww_dfu_one_slot.md)
page: each one is complete on its own, with the flash layout, the flow, what happens on a power cut, and the real
exchange recorded from its test.

## Overview

An update is a **plan**: a header (which image it produces, for which flash geometry, signed) and a list of steps that
turn what the device holds into the new image:

- **ERASE** an erase block,
- **COPY** a chunk the device already has (a delta: unchanged code is not sent again),
- **WRITE** a chunk whose data comes with the step.

A plain full image is a plan made only of ERASE and WRITE, so a node that doesn't do deltas implements the same traits
and simply never sees a COPY. The plan, manifest and descriptor are records of the chunky protocol, defined in octets by
its [`SPEC.md`](https://github.com/romixlab/chunky_mcu_updater/blob/main/SPEC.md) (a draft) and implemented by
[`chunky_format`](https://github.com/romixlab/chunky_mcu_updater/blob/main/chunky_format/src/lib.rs); `ww_dfu` only
carries them, as raw bytes. See [the other side: chunky](ww_dfu_internals.md#the-other-side-chunky).

| Trait         | What it is for                                                                     | Implemented by                   |
|---------------|------------------------------------------------------------------------------------|----------------------------------|
| `UpdateNode`  | Who this node is, its versions and state, and which of the other traits it offers. | every node                       |
| `BootControl` | Boot state, confirm, revert, reboot into the app or the updater.                   | every node                       |
| `Inventory`   | What the node holds now, chunk by chunk (the manifest the host plans against).     | the node that receives the plan  |
| `Stage`       | Receive a plan step by step, apply or store it, finish it; session control.        | the node that receives the plan  |
| `StageBulk`   | Receive a whole plan as one stream on a credit sink, one acknowledgement at the end. | the node that receives the plan on a slow link |

A host always starts with `UpdateNode::node`: one call tells it the profile, versions, boot state, the manifest
digest and which traits are there (`Offers`). On a slow link that is often the only call before the update.

### The scenarios

Each scenario has a code, `<slots><switch>-<receiver>-<delivery>`, and a two-word name:

- **slots and switch**: `2C` two slots, the bootloader **c**opies the new image into place; `2R` two slots, the
  hardware **r**emaps the boot address; `1S` one slot, the plan is **s**tored and applied by the bootloader; `1L` one
  slot, applied **l**ive by an updater.
- **receiver**: `APP` the running application receives the plan; `UPD` an updater does (a bootloader with a
  transport, or a recovery app).
- **delivery**: `STEP` one call per step (`Stage`); `BULK` the whole plan as one stream (`StageBulk`).
- **name**: the mode (*Swap*, *Flip*, *Parcel*, *Pit stop*) and the delivery (*Shuttle*: back and forth for every
  step; *Freight*: one load, one receipt).

| Code                                     | Name             | Flash                       | Receives | Delivery     | Bootloader transport | Downtime                 | Revert | Test link   |
|------------------------------------------|------------------|-----------------------------|----------|--------------|----------------------|--------------------------|--------|-------------|
| [2C-APP-STEP](ww_dfu_ab.md#2c-app-step-swap-shuttle) | Swap Shuttle     | A/B, bootloader copies      | app      | step by step | none                 | one slot copy at reboot  | yes    | USB 64 B    |
| [2C-APP-BULK](ww_dfu_ab.md#2c-app-bulk-swap-freight) | Swap Freight     | A/B, bootloader copies      | app      | bulk         | none                 | one slot copy at reboot  | yes    | UDP         |
| [2R-APP-STEP](ww_dfu_ab.md#2r-app-step-flip-shuttle) | Flip Shuttle     | A/B, hardware remap         | app      | step by step | none                 | a reboot                 | yes    | WebSocket   |
| [2R-APP-BULK](ww_dfu_ab.md#2r-app-bulk-flip-freight) | Flip Freight     | A/B, hardware remap         | app      | bulk         | none                 | a reboot                 | yes    | USB 512 B   |
| [1S-APP-STEP](ww_dfu_one_slot.md#1s-app-step-parcel-shuttle) | Parcel Shuttle | one slot + plan area        | app      | step by step | none                 | applying the plan        | no     | USB 64 B    |
| [1S-APP-BULK](ww_dfu_one_slot.md#1s-app-bulk-parcel-freight) | Parcel Freight | one slot + plan area        | app      | bulk         | none                 | applying the plan        | no     | UDP         |
| [1L-UPD-STEP](ww_dfu_one_slot.md#1l-upd-step-pit-stop-shuttle) | Pit-stop Shuttle | one slot                | updater  | step by step | yes (or recovery app) | download and apply      | no     | USB 64 B    |
| [1L-UPD-BULK](ww_dfu_one_slot.md#1l-upd-bulk-pit-stop-freight) | Pit-stop Freight | one slot                | updater  | bulk         | yes (or recovery app) | download and apply      | no     | WebSocket   |

The other combinations make no sense, which is why they have no code:

- **A/B with a stored plan** (`2x` + `Stored`): the inactive slot already is the place to put the update; storing a
  plan to apply it in place later costs a plan area and gives up the revert. An A/B node answers
  `Error::Unsupported` (`StageLimits::stored` is false).
- **A/B received by an updater** (`2x-UPD`): the app can write the inactive slot while it runs; rebooting into an
  updater only adds downtime and a second transport.
- **One slot, live, received by the app** (`1L-APP`): the app would overwrite the code it runs from. An app that copies
  itself to RAM to do this is an updater.
- **One slot, stored, received by an updater** (`1S-UPD`): an updater with a transport can apply live, and resumes
  after a power cut just as well; storing first only costs the plan area.
- **Full image** is not a scenario of its own: a plan of ERASE and WRITE steps works in all eight. Use it when the host
  has no manifest for the device (a development build) or the delta isn't smaller. [2R-APP-STEP](ww_dfu_ab.md#2r-app-step-flip-shuttle)
  shows one.
- **Bootloader or recovery app** as the updater is not a scenario either: the flow is the same, only
  `NodeInfo::role` differs (`Bootloader` or `Recovery`).

### Picking yours

1. **Does the flash hold two images?**
    - Yes, and the chip or its boot ROM maps either bank to the boot address (dual-bank flash with bank swap on
      STM32 H7, G4, L4; RP2350 partition tables; ESP32 OTA partitions; nRF dual bank): **2R** (*Flip*).
    - Yes, but the image always runs from the same address (single-bank parts with embassy-boot, STM32 F1, G0):
      **2C** (*Swap*).
    - No: go on.
2. **One slot. Must the device keep working during the download, or can't the bootloader have a transport?** (A
   download over LoRa can take hours; a bootloader in 8 KB has no room for USB.)
    - Yes, and there is flash for a plan area as large as the largest plan: **1S** (*Parcel*).
    - No, the bootloader (or a write-protected recovery app) has a transport: **1L** (*Pit stop*).
3. **How long is a round trip?**
    - Short (USB, Ethernet, WebSocket on a LAN, RTT): **STEP** (*Shuttle*). Errors are named per step, up to
      `StageLimits::in_flight` calls can be outstanding.
    - Long or metered (LoRa, LTE-M, satellite, a gateway that stores and forwards): **BULK** (*Freight*). The plan
      flows on a credit sink, never more than the device has room for, one acknowledgement at the end.

### What every scenario shares

- **The device trusts nothing it receives.** It checks every index and offset against its own layout before touching
  flash (`Error::OutOfRange`), every COPY source and every WRITE against the hash in its step
  (`Error::SourceMismatch`, `Error::HashMismatch`), the plan header against its geometry, its version floor and the
  release signature before the first step (`Error::WrongGeometry`, `Error::BelowFloor`, `Error::BadSignature`), and
  the finished image against the signed descriptor before it is staged (`Error::VerifyFailed`). Per-chunk hashes are
  not a trust boundary on their own; the signed descriptor over the whole image is. Chunky's device core refuses
  what needs no key (magic, format, signature scheme, reserved bytes, image length) on the raw bytes before it
  verifies the signature, so a junk plan never costs a signature check.
- **A session is identified by its plan header.** `Stage::begin` (or `StageBulk::begin_bulk`) with the header of an
  unfinished session does not start over: it answers `Session { resumed: true, resume_from }` and the host continues
  from there.
- **Step order.** A step ahead of its turn is `Error::OutOfOrder(expected)`; a step the node already has is accepted
  again and skipped (its reply was lost), so resending is always harmless.
- **Reboots.** `BootControl::reboot` answers before the reset; the host then reconnects to the same device (USB serial
  number, IP address) and finds whatever firmware answers there. When the updater is a boot ROM that doesn't speak
  WireWeaver (RP2350 BOOTSEL), see [Following a reboot](../transport/follow_reboot.md).

## More

- [A/B scenarios](ww_dfu_ab.md): 2C (*Swap*) and 2R (*Flip*), step by step and bulk.
- [One-slot scenarios](ww_dfu_one_slot.md): 1S (*Parcel*) and 1L (*Pit stop*), step by step and bulk.
- [Transcripts and chunky](ww_dfu_internals.md): how the recorded exchanges are made and checked, and the chunky
  records (descriptor, plan, manifest) `ww_dfu` carries.
- The code of the API itself: [`ww_stdlib/ww_dfu/src/lib.rs`](https://github.com/vhrdtech/wire_weaver/blob/master/ww_stdlib/ww_dfu/src/lib.rs).

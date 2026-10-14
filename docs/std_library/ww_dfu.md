# ww_dfu: firmware updates

`ww_dfu` is the firmware update API every WireWeaver device shares: a handful of small traits a node picks from, and
eight ways to put them together, depending on how much flash the device has, who receives the update and how fast
the link is. This page is organized by those eight **scenarios**. Read the overview, pick your scenario with the
questions below, then read only its section: each one is complete on its own, with the flash layout, the flow, what
happens on a power cut, and the real exchange recorded from its test.

## Overview

An update is a **plan**: a header (which image it produces, for which flash geometry, signed) and a list of steps that
turn what the device holds into the new image:

- **ERASE** an erase block,
- **COPY** a chunk the device already has (a delta: unchanged code is not sent again),
- **WRITE** a chunk whose data comes with the step.

A plain full image is a plan made only of ERASE and WRITE, so a node that doesn't do deltas implements the same traits
and simply never sees a COPY. The plan, manifest and descriptor formats are defined in octets by the chunky protocol
spec (`SPEC.md` in `chunky_mcu_updater`); `ww_dfu` only carries them, as raw bytes.

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
| [2C-APP-STEP](#2c-app-step-swap-shuttle) | Swap Shuttle     | A/B, bootloader copies      | app      | step by step | none                 | one slot copy at reboot  | yes    | USB 64 B    |
| [2C-APP-BULK](#2c-app-bulk-swap-freight) | Swap Freight     | A/B, bootloader copies      | app      | bulk         | none                 | one slot copy at reboot  | yes    | UDP         |
| [2R-APP-STEP](#2r-app-step-flip-shuttle) | Flip Shuttle     | A/B, hardware remap         | app      | step by step | none                 | a reboot                 | yes    | WebSocket   |
| [2R-APP-BULK](#2r-app-bulk-flip-freight) | Flip Freight     | A/B, hardware remap         | app      | bulk         | none                 | a reboot                 | yes    | USB 512 B   |
| [1S-APP-STEP](#1s-app-step-parcel-shuttle) | Parcel Shuttle | one slot + plan area        | app      | step by step | none                 | applying the plan        | no     | USB 64 B    |
| [1S-APP-BULK](#1s-app-bulk-parcel-freight) | Parcel Freight | one slot + plan area        | app      | bulk         | none                 | applying the plan        | no     | UDP         |
| [1L-UPD-STEP](#1l-upd-step-pit-stop-shuttle) | Pit-stop Shuttle | one slot                | updater  | step by step | yes (or recovery app) | download and apply      | no     | USB 64 B    |
| [1L-UPD-BULK](#1l-upd-bulk-pit-stop-freight) | Pit-stop Freight | one slot                | updater  | bulk         | yes (or recovery app) | download and apply      | no     | WebSocket   |

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
  has no manifest for the device (a development build) or the delta isn't smaller. [2R-APP-STEP](#2r-app-step-flip-shuttle)
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
  not a trust boundary on their own; the signed descriptor over the whole image is.
- **A session is identified by its plan header.** `Stage::begin` (or `StageBulk::begin_bulk`) with the header of an
  unfinished session does not start over: it answers `Session { resumed: true, resume_from }` and the host continues
  from there.
- **Step order.** A step ahead of its turn is `Error::OutOfOrder(expected)`; a step the node already has is accepted
  again and skipped (its reply was lost), so resending is always harmless.
- **Reboots.** `BootControl::reboot` answers before the reset; the host then reconnects to the same device (USB serial
  number, IP address) and finds whatever firmware answers there.

## 2C-APP-STEP: Swap Shuttle

*A/B with a copying bootloader; the running app receives the plan one call per step over a fast link.*

**Setup.** Two slots of the same size; the image always runs from the active one, and the bootloader copies the new
image into place at the next reset, page by page through a swap page, so a cut during the copy resumes (embassy-boot
on STM32 F1, G0 and other single-bank parts).

```text
| bootloader (no transport) | ACTIVE slot: runs here | DFU slot: the update goes here | swap / state pages |
```

The app implements `UpdateNode`, `BootControl`, `Inventory` and `Stage` (`Profile::AB(SlotSwitch::Copy)`). The
bootloader has no transport.

**Choose it when** the flash holds two images, the part can't remap banks, and the link answers within milliseconds
(USB, Ethernet, a LAN). The device keeps running during the whole download.

**Flow.**

1. `UpdateNode::node`: profile, version, boot state, manifest digest. A known digest means the host knows every chunk
   and can plan a delta; otherwise it reads `Inventory::chunks`.
2. `Stage::limits`: `max_piece` (chunks larger than this come in pieces), `in_flight`.
3. `Stage::begin(Target::Live, header)`: every check that can be made before writing (format, geometry, version floor,
   signature, boot state).
4. `Stage::erase` / `copy` / `write` for every step, into the DFU slot; nothing the app runs from is touched.
5. `Stage::finish`: the image in the DFU slot is checked against the signed descriptor and marked `Staged`.
6. `BootControl::reboot(BootTarget::App)`: the bootloader swaps the slots and starts the new image on **trial**.
7. The host reconnects; the new image checks what matters to it and calls `BootControl::confirm`. Without a confirm,
   the next reset swaps back (`BootState::Reverted`).

**Failures and power cuts.** A lost link or a cut during the download costs only time: the DFU slot is scratch. A cut
during the swap is resumed by the bootloader from its swap state. A refused step changes nothing and names why.

**Resume.** After a lost link, `begin` with the same header answers `resumed: true, resume_from: <next step>`. After
a reset during the download the session is gone (it lives in RAM); the host starts over.

**What the device checks** in this scenario: a COPY source hash before copying, each WRITE's chunk hash once all its
pieces are in, every index against the slot, and the whole image against the descriptor in `finish`.

#### Transcript

The host plans a delta from version 1 to 2 (four ERASE, three COPY, four WRITE), resends one step as if its reply
was lost, finishes, reboots, and confirms the trial image. Test: `swap_shuttle_2c_app_step` over simulated USB, 64 B
packets.

--8<-- "tests/dfu_scenarios/transcripts/2C-APP-STEP.md"

## 2C-APP-BULK: Swap Freight

*A/B with a copying bootloader; the running app receives the whole plan as one stream over a slow link.*

**Setup.** Two slots of the same size; the image always runs from the active one, and the bootloader copies the new
image into place at the next reset, page by page through a swap page, so a cut during the copy resumes (embassy-boot
on single-bank parts).

```text
| bootloader (no transport) | ACTIVE slot: runs here | DFU slot: the update goes here | swap / state pages |
```

The app implements `UpdateNode`, `BootControl`, `Stage` and `StageBulk` (`Inventory` too if hosts may not know its
manifest). The bootloader has no transport.

**Choose it when** the flash holds two images, the part can't remap banks, and a round trip takes long (LoRa, LTE-M,
satellite, a store-and-forward gateway): a call per step would cost more than the delta saves.

**Flow.**

1. `UpdateNode::node` (and `Stage::limits` for `max_piece`, unless the host knows the device type).
2. `StageBulk::begin_bulk(Target::Live, header, total_len, ack_every)`: the checks of `Stage::begin`; `ack_every: 0`
   asks for one acknowledgement at the end.
3. The host opens the `plan` credit sink and sends the plan after its header as `PlanSegment { offset, data }`. The
   device grants credit in bytes as it frees its buffer, so the host never sends more than fits; lost segments are
   sent again from the device's position (go-back-N). The device applies the steps into the DFU slot as they arrive.
4. When all bytes are in, the device sends `PlanAck { have: total_len, status: Complete }` on `plan_ack`.
5. `Stage::finish`: checked against the descriptor, `Staged`.
6. `BootControl::reboot(BootTarget::App)`: the bootloader swaps the slots and starts the new image on trial; the host
   reconnects and calls `BootControl::confirm`.

**Failures and power cuts.** A lost link or a cut during the download costs only time: the DFU slot is scratch. A
step that fails ends the session with `PlanAck { status: Failed(error) }`; a segment that doesn't continue what the
device has is answered with `BulkStatus::Gap` and the position to resend from. A cut during the swap is resumed by
the bootloader.

**Resume.** `begin_bulk` with the same header answers `resumed: true` and `resume_from` in **bytes** after the header;
the host sends from there. The transcript shows a host that restarts half way.

**What the device checks**: each segment's offset against what it has, each step as it completes (COPY source hash,
WRITE hash, index ranges), the image against the descriptor in `finish`.

#### Transcript

The host sends three segments, restarts (a new connection), resumes at byte 192, gets one `Complete`, finishes,
reboots and confirms. Test: `swap_freight_2c_app_bulk` over UDP on localhost.

--8<-- "tests/dfu_scenarios/transcripts/2C-APP-BULK.md"

## 2R-APP-STEP: Flip Shuttle

*A/B with a hardware remap; the running app receives the plan one call per step over a fast link.*

**Setup.** Two banks or partitions; the hardware or boot ROM maps the chosen one to the address the image runs from,
so nothing is copied and the switch is instant (STM32 H7, G4, L4 bank swap; RP2350 partition table with an A/B pair
and try-before-you-buy; ESP32 OTA partitions; nRF dual bank).

```text
bank 1: | image A: runs (mapped to the boot address) |
bank 2: | image B: the update goes here              |      boot ROM / option bytes choose the bank
```

The app implements `UpdateNode`, `BootControl`, `Inventory` and `Stage` (`Profile::AB(SlotSwitch::Remap)`).

**Choose it when** the part can remap its banks and the link answers within milliseconds. The first boot of the new
image takes no longer than any other boot, and so does a revert.

**Flow.**

1. `UpdateNode::node`, `Stage::limits`.
2. `Stage::begin(Target::Live, header)`.
3. `Stage::erase` / `copy` / `write` for every step, into the inactive bank.
4. `Stage::finish`: checked against the descriptor, `Staged`.
5. `BootControl::reboot(BootTarget::App)`: the bootloader (or boot ROM) maps the other bank and starts it on trial.
6. The host reconnects, `BootControl::boot_state` is `Trial`; the image calls `BootControl::confirm`.

**Failures and power cuts.** Nothing the app runs from is touched until the switch, and the switch is a single
write (an option byte, a partition flag), so a cut or a lost link at any point costs nothing but time. An image that
doesn't confirm is mapped out again at the next reset.

**Resume.** After a lost link, `begin` with the same header continues at `resume_from`; after a reset the host
starts over.

**What the device checks**: COPY source hashes, WRITE chunk hashes, every index against the bank, the image against
the descriptor in `finish`.

#### Transcript

A **plain full image**: four ERASE and seven WRITE, no COPY, so the host needs no manifest. Then the reboot, the
trial and the confirm. Test: `flip_shuttle_2r_app_step` over WebSocket on localhost.

--8<-- "tests/dfu_scenarios/transcripts/2R-APP-STEP.md"

## 2R-APP-BULK: Flip Freight

*A/B with a hardware remap; the running app receives the whole plan as one stream.*

**Setup.** Two banks or partitions; the hardware or boot ROM maps the chosen one to the boot address, nothing is
copied, the switch is instant (dual-bank STM32, RP2350 partitions, ESP32 OTA, nRF dual bank).

```text
bank 1: | image A: runs (mapped to the boot address) |
bank 2: | image B: the update goes here              |      boot ROM / option bytes choose the bank
```

The app implements `UpdateNode`, `BootControl`, `Stage` and `StageBulk` (`Profile::AB(SlotSwitch::Remap)`).

**Choose it when** the part can remap its banks and round trips are long or metered, or simply when the whole plan
should go in one stream (a high-speed link where the host wants to keep the pipe full).

**Flow.**

1. `UpdateNode::node`, `Stage::limits`.
2. `StageBulk::begin_bulk(Target::Live, header, total_len, ack_every: 0)`.
3. The plan after its header on the `plan` credit sink; the device applies steps into the inactive bank as they
   complete.
4. `PlanAck { status: Complete }` on `plan_ack`, then `Stage::finish`: `Staged`.
5. `BootControl::reboot(BootTarget::App)`: the other bank starts on trial.
6. The new image confirms with `BootControl::confirm`, or gives up with `BootControl::revert` (or never confirms):
   the next reset maps the previous bank back, `BootState::Reverted`, `LastResult { outcome: Reverted }`.

**Failures and power cuts.** As for any A/B: nothing the app runs from is touched until the switch, a cut costs only
time. A failed step ends the session with `PlanAck { status: Failed(error) }`.

**Resume.** `begin_bulk` with the same header answers the byte to continue from.

**What the device checks**: segment offsets, each completed step (COPY source hash, WRITE hash, ranges), the image
against the descriptor in `finish`.

#### Transcript

The delta in one stream, one `Complete`, reboot into the trial; the new image fails its own checks and calls
`revert`, the reset maps version 1 back. Test: `flip_freight_2r_app_bulk` over simulated USB, 512 B packets.

--8<-- "tests/dfu_scenarios/transcripts/2R-APP-BULK.md"

## 1S-APP-STEP: Parcel Shuttle

*One app slot and a plan area; the running app stores the plan, step by step, and the bootloader applies it in place
after a reset.*

**Setup.** One app slot, updated in place, and a plan area big enough for the largest plan with its WRITE data. The
bootloader has no transport; it applies a stored plan offline and keeps an apply journal (a bit per step, cleared
without an erase) so a cut while applying resumes.

```text
| bootloader (no transport, applies plans) | APP slot: runs here, rewritten in place | plan area (≥ largest plan) |
```

The app implements `UpdateNode`, `BootControl`, `Inventory` and `Stage` (`Profile::InPlace`,
`StageLimits { stored: true, plan_area }`).

**Choose it when** the flash can't hold a second image but has room for a plan area, the bootloader can't have a
transport, and the downtime may be the apply time only (no revert: the old image is gone once applied).

**Flow.**

1. `UpdateNode::node`. If the host doesn't know the manifest digest (a development build, an image changed by an
   interrupted update), it reads `Inventory::layout` and `Inventory::chunks` and plans against that.
2. `Stage::limits`: `stored: true` and `plan_area`.
3. `Stage::begin(Target::Stored, header)`: also checks that the plan fits the plan area (`Error::PlanTooBig`).
4. `Stage::erase` / `copy` / `write`: stored in the plan area in the plan's byte form, nothing in the app slot changes.
5. `Stage::finish`: the stored plan is checked **before anything is touched**: complete, header signature, every
   COPY source still has its hash, every WRITE's data its hash. The state is `PlanStored`.
6. `BootControl::reboot(BootTarget::App)`: the bootloader applies the plan in place, checks the image against the
   descriptor and starts it. The update is confirmed as applied (`LastResult { outcome: Confirmed }`), there is no
   trial.

**Failures and power cuts.** During the download nothing but the plan area changes; the app keeps running. During the
apply, the bootloader journals every finished step; after a cut it redoes the step in flight and continues (every
step's result is checked by its hash, the WRITE data is still in the plan area). A plan that fails while applying
leaves the app slot unbootable; the device needs an updater then (`Outcome::Recovery`), so keep a recovery path for
this case.

**Resume.** After a lost link or an app reset during the download, `begin` with the same header continues where the
plan area ends (`resume_from`: the next step). After a cut while applying, the journal resumes; the host sees nothing
but a longer reboot.

**What the device checks**: COPY source hashes and WRITE chunk hashes as the steps arrive and again in `finish`, the
plan area size, the header signature; the applied image against the descriptor.

#### Transcript

The host doesn't know the manifest and reads it (`layout`, `chunks`), stores the in-place delta (ten steps, one COPY
from a block not yet erased), reboots; the power fails while the bootloader applies step 5; the journal resumes, and
version 2 boots. Test: `parcel_shuttle_1s_app_step` over simulated USB, 64 B packets.

--8<-- "tests/dfu_scenarios/transcripts/1S-APP-STEP.md"

## 1S-APP-BULK: Parcel Freight

*One app slot and a plan area; the running app receives the whole plan in one stream into the plan area, keeps doing
its job, and the bootloader applies the plan after a reset.*

**Setup.** One app slot, updated in place, and a plan area big enough for the largest plan. The bootloader has no
transport; it applies a stored plan offline, with an apply journal.

```text
| bootloader (no transport, applies plans) | APP slot: runs here, rewritten in place | plan area (≥ largest plan) |
```

The app implements `UpdateNode`, `BootControl`, `Stage` and `StageBulk` (`Profile::InPlace`,
`StageLimits { stored: true, plan_area }`).

**Choose it when** the link is slow or metered and the device must keep working while the download takes hours (a
sensor on LoRa), the flash can't hold a second image, and the bootloader has no transport. This is the scenario that
`ww_dfu`'s bulk path was made for: the bytes after the header are the plan area's content, unchanged.

**Flow.**

1. `UpdateNode::node` (and `Stage::limits`, unless the host knows the device type).
2. `StageBulk::begin_bulk(Target::Stored, header, total_len, ack_every)`: checks the header and that the plan fits.
3. The plan after its header on the `plan` credit sink, written to the plan area as it arrives. The app answers
   anything else meanwhile.
4. `PlanAck { status: Complete }` on `plan_ack`.
5. `Stage::finish`: the stored plan is checked before anything is touched (complete, signature, COPY sources, WRITE
   hashes); `PlanStored`.
6. `BootControl::reboot(BootTarget::App)`: the bootloader applies the plan in place and starts the new image.

**Failures and power cuts.** A cut during the download loses nothing stored: after the reboot, `begin_bulk` with the
same header resumes after the bytes in the plan area. A cut during the apply is resumed from the journal. No revert
once applied.

**Resume.** `begin_bulk` with the same header: `resume_from` is the number of plan bytes already in the plan area.
With `ack_every` the device also reports its position every so many bytes, for very long downloads.

**What the device checks**: segment offsets and the plan area size as bytes arrive; in `finish` the whole stored plan
(complete, signature, COPY source hashes, WRITE hashes); while applying, every step's result by its hash and the image
against the descriptor.

#### Transcript

Half the plan, then the app answers `node` in the middle of the download (still version 1, `Confirmed`), the rest,
one `Complete`, `finish` into `PlanStored`, the reboot and the bootloader's apply. Test: `parcel_freight_1s_app_bulk`
over UDP on localhost.

--8<-- "tests/dfu_scenarios/transcripts/1S-APP-BULK.md"

## 1L-UPD-STEP: Pit-stop Shuttle

*One app slot; the app reboots into an updater (a bootloader with a transport, or a recovery app), which receives the
plan one call per step and applies it in place as it arrives.*

**Setup.** One app slot, no extra flash. The updater is either the bootloader with its own transport (USB, CAN) or a
small write-protected recovery app at its own address; it must come back up after any cut and finish the job.

```text
| bootloader + transport = updater | APP slot: rewritten in place |
| bootloader | recovery app (updater, write-protected) | APP slot: rewritten in place |
```

The app implements `UpdateNode` and `BootControl` only (often just `reboot(BootTarget::Updater)` matters). The
updater implements `UpdateNode`, `BootControl`, `Inventory` and `Stage` (`NodeInfo::role` is `Bootloader` or
`Recovery`).

**Choose it when** the flash has no room for a second image or a plan area, and the bootloader (or a recovery app) can
have a transport. It is the fastest end to end and needs no extra flash, but the device does nothing else from the
reboot into the updater until the new app starts.

**Flow.**

1. To the app: `UpdateNode::node`: `offers` has no `stage`, so the host calls `BootControl::reboot(BootTarget::Updater)`.
2. The host reconnects to the same device; the updater answers, with a different API.
3. `UpdateNode::node`, `Stage::limits`: an updater usually has a small buffer, chunks come in pieces of `max_piece`.
4. `Stage::begin(Target::Live, header)`: from here on the app slot is not bootable until the session finishes
   (`BootState::Updating`); the updater records the session in flash.
5. `Stage::erase` / `copy` / `write` applied in place as they arrive; the planner never copies from a chunk an earlier
   step has overwritten.
6. `Stage::finish`: the new image is checked against the descriptor and marked bootable (`Confirmed`).
7. `BootControl::reboot(BootTarget::App)`: the new app runs.

**Failures and power cuts.** A cut leaves no bootable app: the bootloader sees that and starts the updater again
(`BootState::Updating`, `app_version: None`), and the host continues. A lost link the same. The updater must be able
to resume any session it started, which is why it keeps its session record (header and next step) in flash.

**Resume.** `begin` with the same header: `resumed: true`, `resume_from` the step after the last one that finished;
the step in flight is redone from its first piece (`Stage::progress` shows the piece offset while the link is up).

**What the device checks**: the header (format, geometry, floor, signature) before erasing anything, every index
against the app slot, COPY source hashes, WRITE chunk hashes after the last piece, the image against the descriptor.

#### Transcript

The lean app reboots into the updater, which takes chunks in 32-byte pieces. The power fails half way into step 7
(no reply); the device comes back in the updater with no valid app; `begin` resumes at step 7; the update finishes and
the new app starts. Test: `pit_stop_shuttle_1l_upd_step` over simulated USB, 64 B packets.

--8<-- "tests/dfu_scenarios/transcripts/1L-UPD-STEP.md"

## 1L-UPD-BULK: Pit-stop Freight

*One app slot; the app reboots into an updater, which receives the whole plan as one stream and applies it in place
as the steps complete.*

**Setup.** One app slot, no extra flash; the updater is the bootloader with a transport or a recovery app.

```text
| bootloader + transport = updater | APP slot: rewritten in place |
| bootloader | recovery app (updater, write-protected) | APP slot: rewritten in place |
```

The app implements `UpdateNode` and `BootControl`. The updater implements `UpdateNode`, `BootControl`, `Stage` and
`StageBulk`.

**Choose it when** there is no room for a second image or a plan area, the updater has a transport, and round trips
are long, or the link is fast and the host just wants to stream the plan without waiting on every step.

**Flow.**

1. To the app: `UpdateNode::node`, then `BootControl::reboot(BootTarget::Updater)`.
2. The host reconnects; the updater answers. `UpdateNode::node`, `Stage::limits`.
3. `StageBulk::begin_bulk(Target::Live, header, total_len, ack_every: 0)`: the app slot stops being bootable.
4. The plan after its header on the `plan` credit sink, in segments of at most `max_piece`; the updater parses the
   steps from the stream and applies each in place as soon as it is complete.
5. `PlanAck { status: Complete }` on `plan_ack`, then `Stage::finish`: checked, `Confirmed`.
6. `BootControl::reboot(BootTarget::App)`: the new app runs.

**Failures and power cuts.** A cut leaves no bootable app; the bootloader starts the updater again and the host
resumes. A step that fails ends the session with `PlanAck { status: Failed(error) }`; the device stays in the updater
until a session finishes.

**Resume.** `begin_bulk` with the same header answers the byte after the last applied step (the updater keeps it in
its session record); bytes of a step that was not complete are sent again.

**What the device checks**: segment offsets, the header before erasing anything, each completed step (ranges, COPY
source hash, WRITE hash), the image against the descriptor.

#### Transcript

The lean app reboots into the updater; the in-place delta goes in 32-byte segments in one stream, one `Complete`,
`finish` into `Confirmed`, and the new app starts. Test: `pit_stop_freight_1l_upd_bulk` over WebSocket on localhost.

--8<-- "tests/dfu_scenarios/transcripts/1L-UPD-BULK.md"

## About the transcripts and their tests

Every transcript on this page is recorded by its test in `tests/dfu_scenarios` (`src/scenarios.rs`) and checked
against the file included here: a test fails when the exchange changes, until `just dfu-transcripts` writes the new
one and the change is looked at. So the bytes here are the bytes the current code sends.

- **Two views.** The readable one decodes each `ww_link` message with the API model of the firmware that runs:
  method, arguments, result, sink writes (`⇐`) and stream items (`⇒`), credit and sideband events, with each
  message's size and first bytes. The wire view shows every unit of the medium: USB packets, UDP datagrams or
  WebSocket frames. Both are taken on the device side.
- **Left out:** link setup (`Nop`, `GetDeviceInfo`, `LinkSetup`, `LinkReady`, the WebSocket upgrade), which is the
  same on every connection and carries crate versions that change with every release, and pings. WebSocket frames from
  the host are masked with a random key; they are shown unmasked with the key zeroed.
- **The device** is a simulation with real firmware logic on a simulated flash: two banks with NOR semantics (programming
  only clears bits), 8 chunks of 64 bytes, erase blocks of 128 bytes, a plan area, an apply journal, and power cuts
  injected at a given step. A reset restarts the device thread on the same link endpoint, running the bootloader logic
  first; the host reconnects as it would to a real device. The firmwares are generated `ww_dfu` servers
  (`tests/dfu_app_api`, `tests/dfu_lean_app_api`, `tests/dfu_updater_api`).
- **The plan format** is a small stand-in for chunky's (a 35-byte header with a toy signature; steps `01 block`,
  `02 dst src hash`, `03 dst hash len data`), so the step bytes in the transcripts are readable. Hashes are SHA-256
  truncated to 8 bytes, as in the spec.
- **The links** are the real ones from `ww_device` up: WebSocket and UDP on localhost through `wire_weaver_client`'s
  `ws` and `udp` transports; USB is simulated by `wire_weaver_client::in_process::usb_device`, which runs the real USB
  framing on both ends with full-speed (64 B) or high-speed (512 B) packets, but not USB itself (enumeration, data
  toggles, NAKs). RTT is not used: with a debug probe attached a device is flashed directly, without a bootloader.
- **Changing a scenario's link** is one line, `const LINK: Link = ...` in its test (UART and CAN will join `Link`).

---
level: advanced
---

# ww_dfu: A/B scenarios

The four [`ww_dfu`](ww_dfu.md) scenarios for a flash that holds two images: the update goes into the inactive slot
while the app keeps running, and an image that doesn't confirm itself is reverted at the next reset. **2C** (*Swap*)
when the bootloader copies the new image into place, **2R** (*Flip*) when the hardware or boot ROM remaps the boot
address. [Picking yours](ww_dfu.md#picking-yours) says which one fits; each section below is complete on its own.

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
the host sends from there. The `plan` sink continues its own session too: a host that saved `Sink::cursor()` opens it
again with `plan_resume(cursor)`, which answers how many segments the device has in it (one unit per segment), so
nothing the device already took is sent again and a device that lost the session (a reset) is reported as
`StreamError::SinkSessionLost`: the host starts over. See [credit sinks](../api/credit_sinks.md#resume). The
transcript shows a host that restarts half way.

**What the device checks**: each segment's offset against what it has, each step as it completes (COPY source hash,
WRITE hash, index ranges), the image against the descriptor in `finish`.

#### Transcript

The host sends three segments, restarts (a new connection), resumes at byte 192 and continues the `plan` sink's
session at segment 3, gets one `Complete`, finishes, reboots and confirms. Test: `swap_freight_2c_app_bulk` over UDP on localhost.

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

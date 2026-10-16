---
level: advanced
---

# ww_dfu: one-slot scenarios

The four [`ww_dfu`](ww_dfu.md) scenarios for a flash that holds only one image: the app is rewritten in place and
there is no revert. **1S** (*Parcel*) stores the plan in a plan area while the app runs and lets the bootloader apply
it; **1L** (*Pit stop*) reboots into an updater that applies the plan live as it arrives.
[Picking yours](ww_dfu.md#picking-yours) says which one fits; each section below is complete on its own.

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

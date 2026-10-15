## Unreleased

### ⚠️ Breaking

- The `FirmwareUpdate` stub is replaced by the update API (STD-12): traits `UpdateNode`, `BootControl`, `Inventory`,
  `Stage` and `StageBulk`, each node implementing the ones it needs; three update modes (A/B applied live, in place
  from a stored plan, in place applied live by an updater) and two ways a plan travels (step by step, or in one go
  with acknowledgements only when needed). Signed records of the chunky protocol travel as raw octets. Crate docs
  describe the modes, resuming and what the device checks.
- `StageBulk::plan` is a `#[credit]` sink (API-28): the host sends no more segment bytes than the node has room
  for, so small segments don't take the room of a large one; lost segments are resent by the sink's flow control. `PlanAck::credit` removed, `PlanAck` keeps `have` and
  `status`; `plan_ack` is sent at the end, on an offset gap and every `ack_every` bytes if asked.
- `Profile` is `AB(SlotSwitch)` or `InPlace` (was `Swap`, `BankSwap`, `RpBootrom`, `InPlace`): chip names moved to the
  docs. `SlotSwitch::Copy` (the bootloader copies or swaps slot contents, embassy-boot) or `SlotSwitch::Remap`
  (hardware or a boot ROM maps the chosen slot: bank swap, RP2350 partitions, ESP32 OTA, nRF dual-bank).
- Every variable-size field is bounded with `MaxLen` (SW-19): `Stage::write` data and `PlanSegment::data` by
  `MAX_PIECE_LEN` (1024), plan headers by `MAX_HEADER_LEN` (256), an `Inventory::chunks` page by `MAX_CHUNKS_PAGE`
  (64), `NodeInfo::product` by `MAX_PRODUCT_LEN` (32). Every method's request and reply now has a known largest size
  (`<METHOD>_REQUEST_MAX_LEN` / `_RESPONSE_MAX_LEN`), so a firmware checks its buffer at compile time. A node's own
  `StageLimits::max_piece` stays, at most `MAX_PIECE_LEN`. Raising a bound later is compatible.

### 🚀 Features

- Every fallible method of `BootControl`, `Inventory`, `Stage` and `StageBulk` now lists its `Error` variants with
  `#[errors(..)]` (API-32), checked against `Error` and shown in rustdoc, introspection and the client's docs.
  `Error` doesn't use `#[common] Common(CommonError)` (ww_error): only `Unsupported` and `Busy` would map onto it
  cleanly, and `Error` has no lifetime today, so nesting `CommonError<'i>` would need one threaded through this
  crate's whole public API (every trait method, `StageEvent::Failed`) for a two-variant gain — left as a follow-up,
  not done in this pass.
- Guide: `docs/std_library/ww_dfu.md` (<https://ww.vhrd.tech/std_library/ww_dfu/>) names the eight update scenarios
  (`2C-APP-STEP` Swap Shuttle, `2C-APP-BULK` Swap Freight, `2R-APP-STEP` Flip Shuttle, `2R-APP-BULK` Flip Freight,
  `1S-APP-STEP` Parcel Shuttle, `1S-APP-BULK` Parcel Freight, `1L-UPD-STEP` Pit-stop Shuttle, `1L-UPD-BULK` Pit-stop
  Freight), says which combinations make no sense and why, helps pick one, and describes each with its flash layout,
  flow, power-cut behaviour and the exchange recorded from its end-to-end test (`tests/dfu_scenarios`). The crate docs
  link to it first; the stored-plan mode resumes from an apply journal.

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

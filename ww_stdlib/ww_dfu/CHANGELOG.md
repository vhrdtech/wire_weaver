## Unreleased

### 🚀 Features

- `StageLimits::max_piece` is `Bytes<u16, Range<1, { MAX_PIECE_LEN as i64 }>>` (P2401#13nov-0531): the bound names `MAX_PIECE_LEN` instead of repeating 1024. The same type, bytes and wire line (`w2.0`); the 0.1.0 snapshot is saved again for the changed docs.

### ⚠️ Breaking

- Typed numbers, on a new wire line `w2.0` (EVO-65, the P2401 stdlib units survey); the crate depends on `ww_si` (`counting`, `information`). Same bytes, new Rust types: step indices `Index<Step, u32>` (`Stage::erase`/`copy`/`write`'s `index`, `Progress::next_step`, `Error::OutOfOrder`) and counts `Count<Step, u32>` (`Session::steps`, `Progress::steps`, `StageEvent::Applying`); chunks `Index<Chunk, u16>` (`Inventory::chunks`' `first`, `copy`'s `dst`/`src`, `write`'s `dst`) and `Count<Chunk, u16>` (`chunks`' `count`, `StageEvent::Hashing`); erase blocks `Index<Block, u16>` (`erase`'s `block`); byte positions `Index<Byte, u32>` (`write`'s `offset`, `Progress::chunk_offset`, `PlanSegment::offset`, `PlanAck::have`); sizes `Bytes<u32>` (`begin_bulk`'s `total_len`, `SlotLayout::chunk_size`/`erase_size`, `StageEvent::Verifying`). Renamed: `SlotLayout::chunk_count` to `chunks: Count<Chunk, u16>`, `erase_count` to `erase_blocks: Count<Block, u16>`, `Progress::plan_bytes` to `plan_offset: Index<Byte, u32>`, `NodeInfo::geometry_id` to `geometry: GeometryId`. Build values with `Index::new(n)` / `Fixed::new(n)`, read them with `.get()`.
- Wire breaks of `w2.0`: `NodeInfo`'s `product_id`, `app_version` and `version_floor` are replaced by `app: Option<ImageInfo>` (`product: ProductId`, `version: ImageVersion`, `security_counter: SecurityCounter`, `None` without a valid image) and `security_floor: SecurityCounter`: the floor is on the image's anti-rollback counter (chunky's `security_counter`), not on its release version, and a host now sees the running image's counter. `LastResult::version` is `Option<ImageVersion>` (was `0` for none). `Session::resume_from` is split into `resume_step: Index<Step, u32>` (for `Stage`) and `resume_byte: Index<Byte, u32>` (for `StageBulk`). `StageLimits::max_piece` is `Fixed<Byte, One, u16, Range<1, 1024>>` (was a `u32`) and `in_flight` `Fixed<Request, One, u8, Range<1, 255>>`, both refused out of range on decode. `begin_bulk`'s `ack_every` and `StageLimits::plan_area` are `AckEvery` / `PlanArea`, `Niched<Bytes, Null<0>>`: `None.into()` for "only at the end" / "no plan area", still a `0` on the wire. `Error::BelowFloor` means the security counter is below the floor.
- New newtypes, each its `u32` on the wire (`transparent`): `GeometryId`, `ProductId`, `ImageVersion`, `SecurityCounter`; new struct `ImageInfo`.
- `NodeInfo` carries `product_id` (the running image's, from its descriptor) and `seal` (`Seal::Unsigned` or
  `Seal::Ed25519`, how the node checks an image's seal), so a host seals a new image for the device from the one
  `UpdateNode::node` call (STD-12, for `ww dfu flash`, CLT-33).
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

- `BootControl`, `Stage` and `StageBulk` are `#[claim(exclusive)]` (SRV-34): an updater can claim them through the hub (a whole update claims the device's root). Wire compatible, only the introspection data changes.
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
- Wire version `w1.0` of its own (EVO-26), in `Cargo.toml` as `[package.metadata.wire_weaver] wire = "w1.0"`: `#[since]` in this crate names its revisions, and a Rust-only break of the crate no longer touches the wire line of the APIs using it. The snapshot carries it (`wire`, `ext_wire`), so its hash changed.

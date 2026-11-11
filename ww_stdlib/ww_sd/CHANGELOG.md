## Unreleased

### 🚀 Features

- New crate (STD-40): `SdCard`, raw access to an SD card over WireWeaver.
  - `capabilities` (`Capabilities`: `Interface`, widest bus, `max_clock` as `Fixed<Hertz, Kilo, u32>`), `state`
    (`CardState`, `CardKind`, `BusState` with `clock` the same typed kHz, `BusWidth`, `SpeedMode`), the `detect`
    stream, and `registers` (`SdRegisters`: CID, CSD, OCR, an `Rca` newtype, SCR, SD Status as raw bytes).
  - `reinit(lease, max_clock, bus_width)` with a per-step `InitReport` (`InitStep`: command, response, `Outcome`,
    `repeats` a `Count<Item, u16>`, `time` a `Micros<u32>`; `InitReport::total` too), under the `ww_block` `Lease`.
  - `command`: one `RawCommand` (index, argument, application command, `ResponseType`, `DataPhase` with `block_len`
    a `Bytes<u16>` and `blocks` a `Count<Block, u16>`, `timeout` an `Option<Millis<u32>>`) answered by a
    `RawResponse` (`Outcome`, response bytes, `CardStatus` bits, data, `Timing` with `response`/`data`/`busy` as
    `Option<Micros<u32>>`). What the card did is in the outcome, so bytes and timing come back on a failure too.
  - `access` / `RawCommand::access` sort commands into `Access::Free`, `Lease` (the `ww_block` `Lease`) and
    `Restricted` (CMD42, CMD27), and `check_access` is the device's rule: restricted commands are refused unless the
    firmware builds with the feature `raw-sd-unrestricted` (`UNRESTRICTED`, `Capabilities::unrestricted`).
  - `SdError` with the `Common` variant and `NoCard`, `BadLease`, `Restricted`.
  - Every number that means a clock rate, a time, a byte length or the lease is typed (`ww_si`'s `Fixed`, `Micros`,
    `Millis`, `Bytes`, `Count`, and `ww_block`'s `Lease`) rather than a bare integer (P2401#11nov-0947b): the same
    bytes on the wire (checked by a unit test and by `tests/block_sd`'s round trip over the real wire). The crate
    depends on `ww_si` (`time`, `information`, `counting`) and on `ww_block` for `Lease`. `ww api check` currently
    misreports `reinit`'s and `command`'s `lease` argument as a wire break asking for a new line (w2.0): a
    `wire_weaver_evolution`/`wire_weaver_core` walker gap reusing a `transparent` newtype across a crate boundary in
    argument position (`ww_timebase`'s non-transparent `ww_date_time::DateTime` the same way checks clean); filed as
    P2401#11nov-1928, fixed by session `evo-transparent-arg`. By hand, the bytes are unchanged, so the real bump this
    change needs is the wire revision (w1.0 -> w1.1), not a new line; the snapshot is saved once the walker fix
    lands, not before.
- Wire version `w1.0` of its own (EVO-26), in `Cargo.toml` as `[package.metadata.wire_weaver] wire = "w1.0"`: `#[since]` in this crate names its revisions, and a Rust-only break of the crate no longer touches the wire line of the APIs using it. The snapshot carries it (`wire`, `ext_wire`), so its hash changed.

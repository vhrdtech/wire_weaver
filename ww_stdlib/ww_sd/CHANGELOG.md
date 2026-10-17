## Unreleased

### 🚀 Features

- New crate (STD-40): `SdCard`, raw access to an SD card over WireWeaver.
  - `capabilities` (`Capabilities`, `Interface`), `state` (`CardState`, `CardKind`, `BusState`, `BusWidth`,
    `SpeedMode`), the `detect` stream, and `registers` (`SdRegisters`: CID, CSD, OCR, RCA, SCR, SD Status as raw
    bytes).
  - `reinit` with a per-step `InitReport` (`InitStep`: command, response, `Outcome`, repeats, time), under the
    `ww_block` lease.
  - `command`: one `RawCommand` (index, argument, application command, `ResponseType`, `DataPhase`, timeout)
    answered by a `RawResponse` (`Outcome`, response bytes, `CardStatus` bits, data, `Timing`). What the card did is
    in the outcome, so bytes and timing come back on a failure too.
  - `access` / `RawCommand::access` sort commands into `Access::Free`, `Lease` (the `ww_block` lease) and
    `Restricted` (CMD42, CMD27), and `check_access` is the device's rule: restricted commands are refused unless the
    firmware builds with the feature `raw-sd-unrestricted` (`UNRESTRICTED`, `Capabilities::unrestricted`).
  - `SdError` with the `Common` variant and `NoCard`, `BadLease`, `Restricted`.

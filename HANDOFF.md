# Handoff: IO family review round 1 (P2401#15oct-1040)

One commit per review point on branch s/15oct2026-ww-io-review1.

## Done (commits on the branch)
- (1) Hertz everywhere, new `ww_si::Baud` for baud rates. ww_can_bus left as is (old prototype, not in the family).
- (2) `MAX_WATCHES = 512`, `max_watches: u32`, slots `u32`.
- (3) `Matches { bytes: [I2cMatchByte { mask, value }] }`, `sized`.
- API-35 filed in FEATURES (stream fn + borrowed error codegen bug).
- (5) `configure(XConfig) -> Result<XConfig, E>` for UART, I2C, SPI, GPIO bank, 1-Wire; `io_level: Option<Volt>`
  inside; capabilities `io_levels` + `io_level_min/max` everywhere.
- (7) `Uart::monitor`, `Spi::monitor` as `#[optional]` stream fns; traits removed; I2cMonitor kept.
- (6) `RxChunk::end`, head time always set, `rx_timestamps` gone, Time section.

## Next
- (4) CommonError in every IO error enum. Started for ww_i2c: patch in the job tmp dir (point4-i2c.patch, from
  before point 5; re-do by hand, configure lines changed). Mapping: Unsupported/UnsupportedMode/UnsupportedIoLevel
  -> Common(Unsupported); InvalidArgument/MalformedRequest/ZeroLengthTransfer/UnsupportedSpeed -> InvalidArgument;
  InUse; NoIoLevel -> NoPower; Refused; WrongMode -> WrongState; Busy; Timeout; drop CustomU8/U32/NotImplemented.
  Rename crate errors to `<Crate>Error` (GpioError, UartError). `#[errors(..)]` on methods (not on stream fns or
  properties: codegen rejects it there, keep prose). Stream-fn handlers return `E<'static>` with `// TODO(API-35)`.
- Then: FEATURES STD-13 note, just test / lint / check, remove HANDOFF.md, tpm work ready.

## Gotchas
- Snapshots: `just save-snapshots --force` (nothing published yet).
- Don't put backticks inside double-quoted shell strings (permission prompt).
- `self_describing` enums can't hold `Hertz` (final_structure) fields.

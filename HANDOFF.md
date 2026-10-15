# Handoff: IO family review round 1 (P2401#15oct-1040)

One commit per review point on branch s/15oct2026-ww-io-review1.

## Done
- (1) Hertz everywhere, new `ww_si::Baud` for baud rates (UART, LA UartByte trigger). ww_can_bus left as is
  (not part of the IO family 0.2, its `set_clock(u32)` / `valid_clocks() -> SubTypeKind` is old prototype code).
- (2) `MAX_WATCHES = 512`, `max_watches: u32`, slots `u32`.
- (3) `Matches { bytes: [I2cMatchByte { mask, value }] }`, `sized`.

## Next
- (4) `#[common] Common(CommonError<'i>)` in every IO error enum; drop variants CommonError covers
  (Unsupported, InUse, NoIoLevel -> NoPower, Refused, InvalidArgument/MalformedRequest, WrongMode -> WrongState,
  Timeout, Busy); `# Errors` prose -> `#[errors(..)]` (tests_common makes unlisted variants panic!).
- (5) `UartConfig` + `configure(config) -> Result<UartConfig, E>` returning the actual config; same shape for
  I2C (`I2cConfig { mode, io_level }`) and SPI (`SpiConfig` + `io_level`, drop `SpiActual` and the
  `reference_voltage` property); GPIO bank `reference_voltage` property -> config too.
- (6) UART: `RxChunk::end: Second` (last byte), first byte time in the head, mandatory; docs "Timing" section
  naming `ww_timebase::Clock`; drop `Capabilities::rx_timestamps`.
- (7) `UartMonitor::lines` / `SpiMonitor::frames` -> `#[optional]` stream functions in `Uart` / `Spi`.

## Gotchas
- Snapshots: `just save-snapshots --force` (nothing published yet).
- Don't put backticks inside double-quoted shell strings (permission prompt).
- `self_describing` enums can't hold `Hertz` (final_structure) fields.

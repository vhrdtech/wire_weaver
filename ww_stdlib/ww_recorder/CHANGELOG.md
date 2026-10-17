## Unreleased

### 🚀 Features

- New crate (STD-34): `Recorder` to control a recorder that logs tracks to an SD card or flash, and `Track`, the array
  `track[]` of its tracks (0 is the system track).
  - `Recorder`: `status` (`Status`, `State`: `NoCard`, `NeedsFormat`, `Unbound`, `Formatting`, `Logging`, `Full`,
    `Failed`), `card` (`CardFacts`, `CardKind`, `CardContent`, `CardCheck`, `Rating`), the two-step `prepare_format`
    (`FormatAction`, `FormatOffer` with a one-use token, writes nothing) and `format` (card check, then erase),
    `recording_start` / `recording_stop` / `recordings` (`Trigger`, `StopReason`, `RecordingInfo`), `marker`
    (`MarkerKind`), `sync`, the property `settings` (`Settings`: `flush_bytes`, `flush_interval_ms`, `FullPolicy`,
    `RecordingMode`, `UsbView`), `counters` (`RecorderCounters`) and the optional `refresh_usb`.
  - `Track`: `info` (`TrackInfo`: numbers only, the path as resource ids `MaxLen<RefVec<UNib32>, MAX_PATH>`, the item
    type as `ItemType` with its `ww_self` signature and `ww_global` id, the stable key, the rate; names are made on
    the host from the device's API), the properties `enabled`, `durability` (`Durability`) and the optional
    `compression` (`Compression`), and `counters` (`TrackCounters`).
  - `RecorderError` with the `Common` variant and `NoCard`, `BadToken`, `CardFailed`, `NotLogging`,
    `RecordingRunning`, `NoRecording`.

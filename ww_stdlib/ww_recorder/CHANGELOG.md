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
- Optional groups (P2628#17oct-1922): a small firmware can leave a whole tier out with one `omit` / `omit_if` entry
  instead of one call at a time. `#[optional(formatting)]` on `prepare_format` / `format`, `#[optional(recording)]`
  on `recording_start` / `recording_stop` / `recordings`, `#[optional(markers)]` on `marker`,
  `#[optional(track_settings)]` on `Track::durability`, `#[optional(counting)]` on `Recorder::counters` and
  `Track::counters`, `#[optional(usb)]` on `refresh_usb` (was a bare `#[optional]`). `Track::compression` keeps its
  own plain `#[optional]`. The minimum every device keeps: `status`, `card`, `sync`, `settings`, `track[].info` and
  `track[].enabled`. See the table in `docs/std_library/ww_recorder.md`; `tests/recorder_groups` compiles and
  round-trips a server that omits every group.

### 📝 Docs

- What a track is said plainly (Roman asked how a trait can be on a track, P2628#17oct-1922): a track is one stream
  declared in some trait of the device's own API (a `stream!(..)` such as `can[1].rx`), identified by its numeric
  resource path; the recorder logs that stream's items, a host reads them back through the same stream's own
  reliable protocol (backfill), and the data never goes through `Recorder` itself.
- The first user is named the logring engine (repo `logring`, P2628), not `ww_logring`.

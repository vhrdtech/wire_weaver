## Unreleased

### 🚀 Features

- New crate (STD-34): `Recorder` to control a recorder that logs tracks to an SD card or flash, and `Track`, the array
  `track[]` of its tracks (0 is the system track).
  - `Recorder`: `status` (`Status`, `State`: `NoCard`, `NeedsFormat`, `Unbound`, `Formatting`, `Logging`, `Full`,
    `Failed`), `card` (`CardFacts`, `CardKind`, `CardContent`, `CardCheck`, `Rating`), the two-step `prepare_format`
    (`FormatAction`, `FormatOffer` with a one-use token, writes nothing) and `format` (card check, then erase),
    `recording_start` / `recording_stop` / `recordings` (`Trigger`, `StopReason`, `RecordingInfo`), `marker`
    (`MarkerKind`), `sync`, the property `settings` (`Settings`: `flush`, `flush_interval`, `FullPolicy`,
    `RecordingMode`, `UsbView`), `counters` (`RecorderCounters`) and the optional `refresh_usb`.
  - `Track`: `info` (`TrackInfo`: numbers only, the path as resource ids `MaxLen<RefVec<UNib32>, MAX_PATH>`, the item
    type as `ItemType` with its `ww_self` signature and `ww_global` id, the stable key, the rate; names are made on
    the host from the device's API), the properties `enabled`, `durability` (`Durability`) and the optional
    `compression` (`Compression`), and `counters` (`TrackCounters`).
  - `RecorderError` with the `Common` variant and `NoCard`, `BadToken`, `CardFailed`, `NotLogging`,
    `RecordingRunning`, `NoRecording`.
- Optional groups: a small firmware can leave a whole tier out with one `omit` / `omit_if` entry
  instead of one call at a time. `#[optional(formatting)]` on `prepare_format` / `format`, `#[optional(recording)]`
  on `recording_start` / `recording_stop` / `recordings`, `#[optional(markers)]` on `marker`,
  `#[optional(track_settings)]` on `Track::durability`, `#[optional(counting)]` on `Recorder::counters` and
  `Track::counters`, `#[optional(usb)]` on `refresh_usb` (was a bare `#[optional]`). `Track::compression` keeps its
  own plain `#[optional]`. The minimum every device keeps: `status`, `card`, `sync`, `settings`, `track[].info` and
  `track[].enabled`. See the table in `docs/std_library/ww_recorder.md`; `tests/recorder_groups` compiles and
  round-trips a server that omits every group.
- Wire version `w1.0` of its own (EVO-26), in `Cargo.toml` as `[package.metadata.wire_weaver] wire = "w1.0"`: `#[since]` in this crate names its revisions, and a Rust-only break of the crate no longer touches the wire line of the APIs using it. The snapshot carries it (`wire`, `ext_wire`), so its hash changed.

### ⚠️ Breaking

- A new wire line, `w2.0` (EVO-56): every size, duration, rate, id and counter typed instead of a
  bare integer, and `State::Formatting::percent` bounded, per the stdlib units survey
  (`prj/P2401-wireweaver/design/stdlib-units-survey.md`). Crate version stays `0.1.0` (never published).
  - Sizes as `Bytes<T>` / `Mebibytes<u32>`, suffix dropped: `Status::ring_used`, `ring_capacity` (were
    `ring_used_bytes`, `ring_capacity_bytes`), `CardFacts::capacity` (was `capacity_bytes`), `CardCheck::best_flush`
    (was `best_flush_bytes`), `FormatOffer::erases`, `ring` (were `erases_bytes`, `ring_bytes`), `rw_mb_default` /
    `rw_mb_min` / `rw_mb_max` and `format`'s `rw_mb`, `RecordingInfo::bytes`, `Settings::flush` (was `flush_bytes`),
    `TrackCounters::bytes`, `RecorderCounters::bytes_written`.
  - Durations as `Millis<T>` / `Secs<T>`, suffix dropped: `CardCheck::worst_stall` (was `worst_stall_ms`),
    `Settings::flush_interval` (was `flush_interval_ms`), `RecorderCounters::worst_stall`, `FormatOffer::valid_for`
    (was `valid_for_s`), `recording_start`'s `pre_trigger` (was `pre_trigger_s`).
  - `CardCheck::write_rate` (was `write_kib_s`) is `Fixed<ww_si::units::BytePerSecond, Kibi, u32>`.
  - Ids as newtypes: `SessionId` (`Status::session`, `RecordingInfo::session`), `RecordingId` (`Status::recording`,
    `RecordingInfo::id`, `recording_start` / `recording_stop` / `recordings`), `FormatToken` (`FormatOffer::token`,
    `format`'s `token`), `TrackKey` (`TrackInfo::key`), `CardSerial` (`CardFacts::serial`).
  - Counters as `Count<Of, T>` / `Index<Of, T>` of exact_units' counted markers (`Record`, `Segment`, `Block`,
    `Item`, `Session`, `Event`): `TrackCounters::records`, `next` / `durable` / `released`, `dropped`;
    `RecorderCounters::segments_reused` / `ring_segments`, `torn_blocks`, `torn_records`, `dropped`, `card_stalls` /
    `card_errors`, `sessions`.
  - `State::Formatting::percent` is `Fixed<Ratio, Centi, u8, Range<0, 100>>` (was a plain `u8`): a bound narrows the
    wire even though the bytes don't change.
  - `CardFacts::au`, `recording_unit` (were `au_bytes`, `recording_unit_bytes`) and `TrackInfo::rate` are
    `Niched<Fixed<..>, Null<0>>`: `0` was already "unknown" / "not measured" by convention, now decoded as such
    instead of a magic number a reader has to know, same bytes.
  - `ww_si` features gained `information`, `counting`, `dimensionless` next to `time`.

### 📝 Docs

- What a track is said plainly (Roman asked how a trait can be on a track): a track is one stream
  declared in some trait of the device's own API (a `stream!(..)` such as `can[1].rx`), identified by its numeric
  resource path; the recorder logs that stream's items, a host reads them back through the same stream's own
  reliable protocol (backfill), and the data never goes through `Recorder` itself.
- The first user is named the logring engine (repo `logring`), not `ww_logring`.

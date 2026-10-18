# ww_stream changelog

## Unreleased

### ⚠️ Breaking

- `batch::Batcher<T, N, P = DefaultPolicy>`, `Producer`, `Consumer` and `Batched::new` take the codec policy as a
  type (SW-63): `batch::DefaultPolicy` is `CodecPolicy` on `std` and `series::DeltaPacked` on `no_std`, so a
  firmware links only the delta and packed codecs (`DEFAULT_POLICY` is a `DeltaPacked` there). `new(policy)` takes
  any `SeriesPolicy` (a `CodecPolicy` value still works, the type is then `Batcher<T, N, CodecPolicy>`);
  `BatchSource` has an associated `Policy`. Code naming `Batcher<T, N>` / `Batched<S, T, N>` with a `CodecPolicy`
  on `no_std` names the policy: `Batcher<T, N, CodecPolicy>`.
- `batch::Batched<S, T, N, Src = Batcher<T, N>>` (API-41): the batcher may be a `Consumer` split off a batcher whose
  `Producer` runs elsewhere (`Batched::with_consumer`), so an interrupt can push while `poll_streams` encodes; it
  keeps the host's max age within bounds (`grant_max_age`, `max_age_us`, `with_max_age`, `DEFAULT_MAX_AGE_US`,
  `MIN_MAX_AGE_US`, `MAX_MAX_AGE_US`). New `batch::Plain` delivery state with `Consumer::poll_plain`, the
  `BatchSource` and `Push` traits (generated `<stream>_push` takes any `Push`), `batch::Timed` re-exported.

- `rx::Verdict::Accept` has a new field `skip` (API-47): the units at the start of the event the host already has
  (a resend or a resume from inside a multi-unit event); pass on only the units after them. Matches on it name the
  field or use `..`.
- `store::StreamStore::read(pos)` returns the event that holds `pos` (`head.pos <= pos < head.end()`), else the
  first one after it (API-47); before, a position inside an event read the next one. `RamRing`,
  `PersistentRamRing` and `LogStore` do so; a store of your own has to as well.
- `store::StoreState` has a new field `newest_end`: where the newest kept event ends (`next` goes past it when units
  were lost after it, or a recovery cut records off). Stores fill it in; code building a `StoreState` sets it.

### 🐛 Fixes

- Partial overlap of multi-unit events (API-47): a `ResendFrom` or a resume from a position inside an event sends
  that event whole with its own position, without a `lost` (before, the units after the position in it were
  reported lost, or `next_event` underflowed); `rx::Receiver` accepts an event that holds its position with `skip`
  instead of dropping it as a duplicate, and an event partly inside a range the host had before a resume the same
  way. `Receiver::ack` may take a position inside an event.
- `tx::Reliable` no longer treats a `ResendFrom` as an acknowledgement: with `AckMode::Explicit` it freed store data
  the host had received but not confirmed, and a crash in between lost it. `rx::Receiver::poll` sends an `Ack`
  right before a `ResendFrom` when its position moved, so the store is freed as soon as before.
- `tx::Reliable` takes an `Ack` or `ResendFrom` up to the highest position it sent, not only up to where it goes on
  sending: a late `ResendFrom` (reordering link) moved that back below what the host had, and its next `Ack` was
  ignored as an overrun; a backfill run whose rest the store had dropped never ended.
- `tx::Reliable::ready` no longer reports the live run ready after it sent the newest event when the store's `next`
  is past that event (after a power-cut recovery of `PersistentRamRing` cut records off): every poll read the store
  for nothing until the next push. It looks at `StoreState::newest_end`.
- `tx::Reliable::next_event` frees the store up to the live run's acknowledged position when the backfill run ends
  there (the store dropped the rest of the backlog); before, that waited for another `Ack`.

### 🚀 Features

- `batch` module (API-41), the device side of batched streams and observed properties, `no_std` and without an
  allocator:
    - `Batcher<T, N>`: two struct-of-arrays stores (`SeriesStore`) of `N` items (up to `MAX_BATCH`, 8191), shared
      by a `Producer` and a `Consumer` (`split()`) through one atomic word, no lock: the producer (a control loop,
      an interrupt, another core) pushes with a copy of the item and two atomic operations, never an encode or a
      wait (about 62 instructions per `Timed<Ampere>` on a Cortex-M3, 71 for the push that hands a full batch
      over); the consumer encodes a sealed store while the producer fills the other one, then gives it back. A
      batch is sealed at `N` items, when one more item could pass the event budget (a limit the consumer works out
      from the worst case and the last batch's bytes per item), or by the consumer at the host's maximum age or a
      smaller budget. When the spare isn't back the batch is lost, counted by the producer (`Seal::Lost`) and
      reported before the store's next batch (`Encoded::lost_before`, `Polled::lost`). `encode_into` writes the
      sealed batch once as a `Series<T>`, splitting it when it doesn't fit the room. Used from one task, the
      batcher's own methods are those of both halves. `Cause`, `Seal`, `Polled`, `Encoded`, `EncodeError`,
      `BatchStats`, `DEFAULT_BATCH` (32), `DEFAULT_POLICY` (`SMALLEST` with `std`, else `FIXED_DELTA_PACKED`).
      Targets without compare-exchange (`thumbv6m`) need `portable-atomic`'s `critical-section` (or
      `unsafe-assume-single-core`) feature in the firmware.
    - `Scratch<BYTES>` (`scratch_bytes::<T>(n)`): one buffer the items of a batch are rebuilt in while it is
      counted or encoded, shared by every batcher of a server (about 70 instructions per item over the encode).
    - `Outbox<BYTES>`: encoded batches of one trait level's observed properties, oldest first; `push_from` drops
      older entries to make room (counted as missed), `iter`, `first_of`, `remove_first_of`, `pop_oldest`.
    - `Batched<S, T, N>`: a stream with its batcher (`split()` gives the halves and the stream);
      `Consumer::poll_reliable` puts each batch into a `Reliable` store as one event (`units` = items, no head
      `t`, resends copy it), `Consumer::poll_counted` returns the head and bytes of a `Counted` event to send.
- `StreamStore::max_item_len()` (default: no limit), implemented by `RamRing`, `PersistentRamRing` and `LogStore`:
  the longest item a store takes in one event, the budget of a batched reliable stream.
- `StoreState::session` docs: sessions with bit 31 set are reserved for stores that start a stream by themselves
  (a logring boot that didn't recover positions, P2628 S3); hosts and firmware keep it clear.
- `observe` module (SRV-18), the device side of observed properties for generated servers: `ObserveTable<N>` per
  trait level (`apply` validates an `ObserveRequest` in full before changing anything and clamps it to
  `ObserveLimits`, `granted`, `mark_changed`, `reset`), `plan` / `Selection` / `commit` for `poll_properties`
  (token bucket per level, earliest deadline first, missed sends counted), `LevelDesc`, `max_write_len`, `measure`,
  `event_overhead`. Properties have a reserved share of the link (`ObserveLimits::total_budget`), stored streams get
  what `poll_streams` is given.
- For generated code (API-31): `Counted::status()` / `Reliable::status()` (`StreamStatus`), `Reliable::on_host_sideband`
  (an `Ack` / `ResendFrom` where no clock is at hand, the time is taken at the next `poll_query`), `tx::PushItemError`,
  `Second` and `StreamStatus` re-exported.
- First version (API-30), the sans-IO, `no_std` core of counted and reliable streams
  (`docs/api/stream_reliability.md`):
    - `tx::Counted`: positions, `lost`, sessions, resume; `tx::Reliable<S: StreamStore>`: live and backfill runs,
      byte grant with an in-flight table, go-back-N on `ResendFrom`, `Query` when the host is silent, sessions,
      resume by cursor with the latest values first, store release on `Ack`.
    - `rx::Receiver`: in order per run, gaps by cause (`Device`, `Link`, `Store`), duplicates and late events,
      `ResendFrom` once per gap, `Ack` timing, `AckMode::{OnReceive, Explicit}`, cursors.
    - `store::StreamStore` trait (`StoredEvent`, `StoreState`, `PushError`, `FullPolicy`) and `RamRing<N>`, with
      drop newest or drop oldest when full.
    - `sched::Scheduler`: priority classes, live before backfill, round-robin, a byte budget per turn.
    - `counters::StreamCnt`, typed `cnt` instance counters behind the `cnt` feature, and `TxStats`.
- `store::LogStore` (SRV-15), behind the `log-store` feature: a power-cut safe log on a block device for reliable
  streams. Survives reboots and power cuts (at most the record being written is lost, counted in
  `Recovery::torn_records`), keeps session, positions, released position and `dropped_units`, starts a new clock
  epoch on each mount and marks the first event after it (`HeadExt::clock_epoch`); drop newest or drop oldest when
  full. Media through `store::log::LogMedium`: `SdMedium` on an `embedded-sdmmc` `BlockDevice` (feature `sdmmc`),
  `NorMedium` on an `embedded-storage-async` `NorFlash` (feature `nor-flash`), `SimMedium` (`SimSd`, `SimNor`) in
  RAM with power cuts for tests (with `std`).
- `store::PersistentRamRing` (SRV-16), behind the `persistent-ram` feature: a RAM ring in a `store::RamArea` placed
  in `.uninit` RAM that survives a soft reset (fault → `sys_reset`), so the logs before a crash reach the host. Two
  header slots and every record checked by CRC-32C at boot (`RamRecovery`), garbage after a power-on starts a new
  ring; keeps session, positions, released position and `dropped_units`, starts a new clock epoch per boot and marks
  the first event after it (`HeadExt::clock_epoch`). `RamArea::image()` / `load_image()` carry it over a simulated
  reset in tests.

### 🐛 Fixes

- `observe` (SRV-18): a level short of budget and polled often sent one value per update, each with its own header,
  so it put more bytes on the link than one `StreamData` per value would (motor demo, EX-11: 8 values every ms in
  30 kB/s, `poll_properties` every 40 µs: 1432 updates/s of 1.4 values, 7280 missed/s). When not every due property
  fits the bucket and it isn't full, `Selection` now holds (`is_held()`): nothing goes out until they all fit or the
  bucket is full (same case: 666 updates/s of 8 values, 2.7 times the values in the same bytes). A property held
  back or left out is late, not missed: a periodic one counts once per whole period it falls behind, an on-change
  one once it waited its `min`; frequent polls no longer inflate `missed`.
- `observe`: `Plan::t()` is `Second { Micro, U32 }`, the device's microseconds as a wrapping `u32` as stream heads
  have them: the `PropertyUpdate` header is 10 bytes instead of 14 (4 without `t`).

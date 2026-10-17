# ww_stream changelog

## Unreleased

### 🚀 Features

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

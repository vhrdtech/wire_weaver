# ww_stream changelog

## Unreleased

### 🚀 Features

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

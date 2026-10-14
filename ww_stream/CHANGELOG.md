# ww_stream changelog

## Unreleased

### 🚀 Features

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

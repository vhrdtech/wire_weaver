# Handoff: ww-stream-core (API-30, piece 1 of docs/api/stream_reliability.md)

Done:
- Skeleton: `ww_stream` crate (workspace version, `cnt` feature), `store::StreamStore` trait with `StoredEvent`,
  `StoreState`, `PushError`, `FullPolicy`; `ww_client_server::stream::{StreamHead, HeadExt}` types (codec next).

Next: head codec + wire types, tx (Counted, Reliable), RamRing, rx, sched, StreamCnt, ww_self delivery kind.

Decisions:
- Drop-oldest stores report evictions through `state().oldest` moving on (push still returns `Ok`).
- `ww_si::Second` derives `PartialEq` now (StreamHead is PartialEq).

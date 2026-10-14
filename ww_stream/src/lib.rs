#![cfg_attr(not(any(feature = "std", test)), no_std)]
//! Counted and reliable WireWeaver streams (API-30): positions in units, exact loss accounting, acknowledgements
//! and go-back-N resends, stores that keep data while the host is slow or gone, and backfill after a reconnect with
//! the latest values first. Design and reasons: `docs/api/stream_reliability.md`.
//!
//! Everything here is sans-IO: generated server and client code (or a test) feeds the state machines what arrived
//! and sends what they return. The wire types live in `ww_client_server::stream`.
//!
//! | Module | What | Used by |
//! |---|---|---|
//! | [store] | [StreamStore](store::StreamStore) | device |

pub mod store;

pub use ww_client_server::stream::{HeadExt, StreamHead};

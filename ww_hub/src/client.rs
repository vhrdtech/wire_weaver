//! A typed client of the hub's API (`ww_hub_api`), for `ww hub`, tests and programs that look at the hub. It lives
//! in `wire_weaver_client` (its `hub` feature), which uses it to route `connect()`; re-exported here.
//!
//! ```ignore
//! let hub = HubClient::connect().await?;
//! let info = hub.info().call().await?;
//! ```

pub use wire_weaver_client::hub::HubClient;

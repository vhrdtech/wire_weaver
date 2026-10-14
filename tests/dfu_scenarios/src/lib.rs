//! `ww_dfu` scenarios end to end, see `docs/std_library/ww_dfu.md`.
pub mod device;
pub mod firmware;
pub mod flash;
pub mod host;
pub mod node;
pub mod plan;
pub mod record;

#[cfg(test)]
mod scenarios;

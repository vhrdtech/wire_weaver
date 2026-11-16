//! The `ww` commands of the WireWeaver standard library APIs: `board`, `counters`, `dfu` (with the RP and STM32
//! boot ROMs), `pins`, `report`, and what the device contract adds to `info` and `sync`. Each command registers
//! itself with [`wire_weaver_cli::registry`]; a binary that links this crate and `wire_weaver_cli` has them all.

mod board;
mod bootsel;
mod complete;
mod counters;
mod dfu;
mod dfu_client;
mod indication;
mod pins;
mod pins_tui;
mod report;
mod rework_check;
mod rom;
mod stm32dfu;

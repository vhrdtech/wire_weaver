//! The `ww` commands of the WireWeaver standard library APIs: `board`, `counters`, `dfu` (with the RP and STM32
//! boot ROMs), `pins`, `report`, and what the device contract adds to `info` and `sync`. Each command registers
//! itself with [`wire_weaver_cli::registry`]; a binary that links this crate and `wire_weaver_cli` has them all.

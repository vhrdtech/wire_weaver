# ww_std_cli

The `ww` commands of the WireWeaver standard library APIs, as a library: `board`, `counters`, `dfu` (with the RP and
STM32 boot ROMs), `pins`, `report`, and the device parts of `info` and `sync`. Each command registers itself with
`wire_weaver_cli`'s command registry; a binary that links this crate and `wire_weaver_cli` has them all.

# ww_report

Device info and bug report of any WireWeaver device, as a library: firmware, board, ids, counters, log and last reset,
decoded with the device's ELF (`crash` symbolizes a crash record with its DWARF), and the rework check flashing runs
through a boot ROM (`rework_check`). `ww info` and `ww report` (`ww_std_cli`) print what it collects.

Documentation: `docs/std_library/device_contract.md`, `docs/std_library/ww_board_info.md`, `docs/cli_reference.md`.

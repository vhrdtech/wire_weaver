# Changelog

## Unreleased

### 🚀 Features

- `ww dfu flash` runs `ww chunky flash` in process when the binary links `chunky_ww_cli`, else the `ww-chunky` plugin (TOOL-38).
- `ww board rework` and the `ww dfu` commands print a common error the device answered with by its text (STD-16).
- New crate (TOOL-36): `ww board`, `counters`, `dfu`, `pins`, `report` and the device parts of `info` and `sync`, moved from `wire_weaver_cli`, register themselves.

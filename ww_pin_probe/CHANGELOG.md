# Changelog

## Unreleased

### 🚀 Features

- New crate (TOOL-15, host side of STD-47): pin tests on any device serving `ww_pin_diag::PinDiag`, wherever in its API. `probe::run` tests pulls (external pull-up, pull-down, floating, held), shorts to GND and to the supply per point, and shorts between points, only with what each point's caps allow, refusals reported, `release_all` at the end also when cancelled or failed; `select::select` (names, `P<n>` ids, ranges, globs on names and groups); `plan::Plan` (expectations, narrower `allow`, `[shorts] among` / `same_net`) and `plan::save` (writes what a board shows into `expect`, keeping the file's comments); `device::DevicePins` (`Pins` over a `DynClient`, `PinDiag` found by introspection) and `device::Names`; `mock::MockBoard` for tests. Names sent as ids show as `#<id>` until the ELF lookup lands (STD-48).

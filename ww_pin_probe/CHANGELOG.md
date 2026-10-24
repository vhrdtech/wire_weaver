# Changelog

## Unreleased

### 🚀 Features

- New crate (TOOL-15, host side of STD-47): pin tests on any device serving `ww_pin_diag::PinDiag`, wherever in its API. `probe::run` tests pulls (external pull-up, pull-down, floating, held), shorts to GND and to the supply per point, and shorts between points, only with what each point's caps allow, refusals reported, `release_all` at the end also when cancelled or failed; `select::select` (names, `P<n>` ids, ranges, globs on names and groups); `plan::Plan` (expectations, narrower `allow`, `[shorts] among` / `same_net`) and `plan::save` (writes what a board shows into `expect`, keeping the file's comments); `device::DevicePins` (`Pins` over a `DynClient`, `PinDiag` found by introspection) and `device::Names`; `mock::MockBoard` for tests. Names sent as ids show as `#<id>` until the ELF lookup lands (STD-48).
- Live view of every point (TOOL-21, what `ww pins tui` runs): `board::Board` reads a board preset, boardmap's board description as it is (`[board] name`/`geometry`, `[[view.connector]]`, `[aliases]`, `[pins]` roles; pads, positions and nets from its geometry JSON, nets joined across fitted `SB*` solder bridges) or a preset for ww alone with `nets` and `columns` per connector; `Board::point_for` matches points to pads by net, joined net or alias, `Board::columns` gives board order. `live::Layout` (connectors in board order, points on no pad apart, or a panel per group), `live::Cursor` stepping across panel edges, `live::Live` (`read_all`, `command` with `Command::{Input, Drive, Toggle, Weak, ReadAll, Release}`, `release`, refusals as messages). `bank::BankPins`: `Pins` over every `ww_gpio::Bank` of a device (arrays entered by valid indices), modes and pulls put back by `release_all`.

### 🐛 Fixes

- `probe::Setup::new` with no selected points takes the plan's points also when there are watched ones; it took none, so `--plan F --watch X` tested only X (TOOL-15).

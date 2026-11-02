# Changelog

## Unreleased

### 🚀 Features

- `ww info` / `ww report` print the board's reworks (`ww_board_info::BoardInfo::REWORKS`, STD-24) when the device
  serves them: `info::Board::reworks: Part<Vec<Rework>>`. `Part::from_error` maps `NotImplementedByDevice` to
  `NotImplemented`. `info::Rework` follows `ww_board_info` 0.3.0 (STD-98): `id: u16`, `title: Option<String>`; a
  rework the firmware doesn't know prints as `rework 9 (unknown to this firmware): applied, the board runs on its
  safe features until a firmware that knows it`.
- New crate: `collect_info` (`ww info`: firmware, board, ids, label, last reset of any device by global trait path),
  `collect_report` and `render_report` (`ww report`: info, counters, the stored `defmt` log and the crash's addresses
  decoded with the ELF found by build id), `Contract` (clients of every trait on one connection), `find_trait`
  (absolute path of a trait in a device's introspection data, which the log stream needs) (CLT-25). The last reset shows `boots_since_crash` (STD-27).
- `find_all_traits`: every place a trait is implemented at, with the path's names (`diag.pins`), for `ww pins` (TOOL-15).

### 🐛 Fixes

- Drop the unused `serde_json` dependency (P2401, cargo-machete).

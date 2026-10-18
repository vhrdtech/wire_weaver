# Changelog

## Unreleased

### 🚀 Features

- New crate: `collect_info` (`ww info`: firmware, board, ids, label, last reset of any device by global trait path),
  `collect_report` and `render_report` (`ww report`: info, counters, the stored `defmt` log and the crash's addresses
  decoded with the ELF found by build id), `Contract` (clients of every trait on one connection), `find_trait`
  (absolute path of a trait in a device's introspection data, which the log stream needs) (CLT-25). The last reset shows `boots_since_crash` (STD-27).

### 🐛 Fixes

- Drop the unused `serde_json` dependency (P2401, cargo-machete).

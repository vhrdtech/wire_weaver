# Handoff: ww-counters-cnt (STD-25, P2401#15oct-1114c)

Branches `s/15oct2026-ww-counters-cnt` in both worktrees: `wire_weaver/` and `cnt/` next to it.

## Done

cnt (3 commits):
- `cnt`: `ram_used_words()` / `bkp_used_words()`, `SIGNATURE_VERSION`; disabled builds get 0 / empty buffers.
- `cnt_core`: `Source` trait (probe, WireWeaver, tests), `Counters::read` / `read_changes` / `reset`, torn `u64`
  re-read; `elf` module: build id, note location in flash, `resolve` (given → checked; `WW_ELF_PATH`,
  `~/.cache/ww/elf`, `target/`), `add` (ELF or `.tar.zst` / `.tar` release archive into the ELF dir).
- `cnt_view` (new crate): list/read/watch/TUI/JSON views over any `Source`, `tui::draw_once` for tests/embedding.
- `cnt_cli`: `ProbeSource`; read/tui/reset check the target's build id (refuse, `--force`, or switch to the ELF
  with the target's id); `cnt elf add|id`; `--version` with SHA + build time.
- `fixtures/counters_fw`: Cortex-M test firmware, ELF checked in at `cnt_core/tests/fixtures/counters_fw.elf`
  (`just fixtures` rebuilds); copied to wire_weaver `tests/counters/fixtures/`.

wire_weaver (3 commits):
- `ww_counters` reworked (const INFO, read pages of UVlq32, reset, optional watch), `read_words` / `reset_words`,
  feature `host`: `WwSource<D: CountersDevice>`.
- `tests/counters(_api)`: e2e over in-process transport, decoded with the fixture ELF, mismatch refused, TUI frame.
- `ww counters read|watch|tui|reset|info` in wire_weaver_cli.
- docs page, cli.md, FEATURES STD-25, changelogs, snapshots.

## Decisions

- Dependencies one way only (parent's recommendation, Roman asked): wire_weaver → cnt. So no `cnt --ww`; the
  WireWeaver side is `ww counters`, the views live in `cnt_view`. `WwSource` takes a `CountersDevice` (two calls on
  a generated client) because `ww_codegen!` can't generate a client for a trait of its own crate.
- wire_weaver uses `cnt_core` / `cnt_view` as path deps on `../cnt` (root `[workspace.dependencies]`): breaks
  wire_weaver's GitHub CI until cnt 0.5 is published and they switch to `"0.5"`. **Waiting for Roman's answer
  before ready-to-land.**
- Release archive: no index needed, `add` stores every ELF in it by its own build id.

## Next / follow-ups

- Publish cnt 0.5 (needs Roman's OK), switch wire_weaver to crates.io `cnt_core` / `cnt_view` (and `cnt = "0.5"`
  for firmware using `ram_used_words`).
- `ww elf add` (only `cnt elf add` exists); a lookup miss could name the release archive from
  `FirmwareInfo::FIRMWARE.version`.
- `watch` stream: nothing serves it yet; host side would need a `Source` that applies `CounterChanges`.
- cnt CLI: shell completions (CLI quality rule), `just test` / `lint` recipes in the cnt repo.
- Probe path build id check untested on hardware (no probe on omarchy-m1).

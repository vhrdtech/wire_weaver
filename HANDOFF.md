# HANDOFF: dfu-scenarios

Task (14 Oct 2026): ww_dfu docs page with one self-contained section per scenario + one e2e test per scenario with
golden transcripts the docs include. Brief: see the session's first prompt.

## Done (all committed on s/14oct2026-dfu-scenarios)
- `wire_weaver_client::in_process::usb_device`: simulated USB (real USB framing, 64/512 B packets) + test, CHANGELOG.
- `tests/dfu_{app,lean_app,updater}_api`: three API roots.
- `tests/dfu_scenarios`: plan.rs (toy plan format, planners), flash.rs (NOR banks, bootloader, journal, cuts),
  node.rs (trait logic), firmware.rs (generated servers), device.rs (device thread, `Link`, reboots), host.rs
  (clients, `connect!`, `send_steps!`, `send_bulk!`), record.rs (taps, readable + wire transcript, golden check),
  scenarios.rs: all 8 scenarios pass, stable over 30 runs. Goldens: `tests/dfu_scenarios/transcripts/*.md`,
  `just dfu-transcripts` regenerates.
- Codegen fix (own commit): stream items ending mid-byte / with own sizes were broken on the wire (PlanAck never
  decoded); now written standalone with to_ww_bytes. Regression test tests/streams odd_sized_stream. CHANGELOG
  wire_weaver_core.
- docs/std_library/ww_dfu.md rewritten (overview, table, meaningless combos, chooser, 8 sections, snippet includes),
  crate docs link first, README + std overview link, ww_dfu CHANGELOG, FEATURES STD-12, ww_dfu snapshot re-saved.
- `just build-docs` (approved by user, in this worktree only): no issues; anchors checked. site/ removed.
- Clippy for dfu_scenarios fixed; fmt done.

## Next
1. Run `just lint` fully (last full run stopped at clippy errors now fixed), `just test`, `just check`.
   `cargo sort -w -g` (without --check) rewrote ww_device/Cargo.toml and ww_firmware_info/Cargo.toml: unrelated,
   reverted; don't commit those.
2. `git rm HANDOFF.md`, commit.
3. `tpm work ready --note "..."`, `tpm work say parent` with the scenario table:
   2C-APP-STEP Swap Shuttle (USB 64), 2C-APP-BULK Swap Freight (UDP), 2R-APP-STEP Flip Shuttle (WS),
   2R-APP-BULK Flip Freight (USB 512), 1S-APP-STEP Parcel Shuttle (USB 64), 1S-APP-BULK Parcel Freight (UDP),
   1L-UPD-STEP Pit-stop Shuttle (USB 64), 1L-UPD-BULK Pit-stop Freight (WS). Mention the codegen bug fix.

## Gotchas
- Link setup bytes carry crate versions: left out of transcripts on purpose (release bumps would break goldens).
- WS host frames have random mask keys: recorded unmasked with zeroed key; WS tap records before the write (race).
- Crate docs of ww_dfu are part of its API snapshot: `just save-snapshots --force` after editing them (0.1.0 never
  published).

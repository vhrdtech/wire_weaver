# HANDOFF (sink-hw, TEST-26)

## Done
- wire_weaver: `Commander::reconnect()` (EX-10 first part, commit 2d091b83), flush-after-resume fix (6f3f2363).
- typec_dongle (worktree ~/git/.wt/sink-hw/typec_dongle, branch s/19oct2026-sink-hw, on top of s/19oct2026-sink-switch):
  fw header UART `uart[1]` (USART5 PB3 TX = IO7 = header pin 1, PB4 RX = IO8 = pin 2, 921600 8N1, tx credit 2048 B),
  example `donguru-core/examples/uart_resume.rs` (modes plain / reconnect / restart, `--mock` dry run), mock uart[1] loopback.
- Dry run on the mock: OK, no stalls.

## Next (needs the hardware)
- Physical: J-Link + Donguru on omarchy-m1 and a jumper between header pins 1 and 2 (IO7-IO8).
- Flash: `cd fw/donguru && cargo run --release` (probe-rs runner in .cargo/config.toml) or `probe-rs download`; J-Link only for this test.
- Run: `cargo run -p donguru-core --release --example uart_resume -- --mode plain|reconnect|restart`.
- Report numbers with `tpm work say credit-sink-resume`, then FEATURES TEST-26 -> done with numbers, CHANGELOG, `git rm HANDOFF.md`, `tpm work ready`.

## Gotchas
- `cargo fmt` in typec_dongle touches unrelated fw files: revert them (git diff PATH | git apply -R).
- fw build prints a harmless "invalid key-value pair" from the embedded_bedrock template Cargo.toml.

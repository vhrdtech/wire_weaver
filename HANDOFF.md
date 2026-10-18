# Handoff: shared-clock (I-2)

## Done (step 1)
- `shrink_wrap::series::RawPlain` (FixedCodecs: raw + plain), test that FixedCodecs types write their CodecPolicy's bytes, CHANGELOG.
- Benches: `examples_mcu/mcu_qemu/src/{shared_clock,foc_signals}.rs`, bins `qemu-shared-clock-{delta-packed,raw-plain}` (mcu_qemu, mcu_qemu_m4f), `stm32h725ig-shared-clock-cycles` (usb_stm32h725ig).
- `qemu_icount` streams the trace from QEMU's stderr (no file) and stops at `--max-insns` / `--timeout`.
- H725 numbers in ~/git/tpm/prj/P2401-wireweaver/design/batched_streams.md, "Shared clock, measured" (uncommitted in the tpm repo). Parent told.

## Next
- QEMU M3/M4F instruction counts: needs `qemu-system-arm` on omarchy-m1 (root, asked the user). Run in this worktree: `examples_mcu/qemu_icount/target/release/qemu_icount cortex-m3 lm3s6965evb examples_mcu/mcu_qemu/target/thumbv7m-none-eabi/release/qemu-shared-clock-delta-packed`. Brackets: calibration, then per batch 21 timed, 21 untimed, 1 clock. Sum each group.
- Step 2 after piece 7 (batched-observe) is on master: `tpm work rebase`, then SharedClock for +batched observe levels.

## Gotchas
- omarchy-m1 is aarch64. Never run benches on another PC's /tmp.
- The `probe-rs run` bin idles in wfi after "done": stop it when "done" shows up. Don't pipe it into `head`.

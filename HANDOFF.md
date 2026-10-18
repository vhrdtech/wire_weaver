# Handoff: shared-clock (I-2)

## Done (step 1)
- `shrink_wrap::series::RawPlain` (FixedCodecs: raw + plain), test that FixedCodecs types write their CodecPolicy's bytes, CHANGELOG.
- Benches: `examples_mcu/mcu_qemu/src/{shared_clock,foc_signals}.rs`, bins `qemu-shared-clock-{delta-packed,raw-plain}` (mcu_qemu, mcu_qemu_m4f), `stm32h725ig-shared-clock-cycles` (usb_stm32h725ig).
- `qemu_icount` streams the trace from QEMU's stderr (no file) and stops at `--max-insns` / `--timeout`.
- QEMU M3/M4F counts done on gpd-omarchy (user's OK, scratch /tmp/shared-clock-bench removed). Brackets per run: calibration, then per batch 21 timed, 21 untimed, 1 clock.
- H725 and QEMU numbers in ~/git/tpm/prj/P2401-wireweaver/design/batched_streams.md, "Shared clock, measured" (uncommitted in the tpm repo). Parent told.

## Next
- Step 2 after piece 7 (batched-observe) is on master: `tpm work rebase`, then SharedClock for +batched observe levels.

## Gotchas
- omarchy-m1 is aarch64. Never run benches on another PC's /tmp.
- The `probe-rs run` bin idles in wfi after "done": stop it when "done" shows up. Don't pipe it into `head`.

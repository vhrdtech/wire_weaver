# TEST-23 series-encode-cpu: handoff

Task: piece 4 of `prj/P2401-wireweaver/design/batched_streams.md` (CPU/flash of `Series<Timed<Ampere>>` encode
on Cortex-M3/M4F under three codec policies). Plan read and followed; parent session batched-streams@omarchy-m1.

## Done

- Real recorded signal: 64 consecutive FOC ticks of `examples/motor_demo`'s simulator (`Device::new(Options::default())`,
  preset `VescOutrunner`, profile `SpeedSteps`, seed 1, default `Fan` load), `motor.truth().i_a` quantized
  `Ampere::quantize(.., Prefix::Milli, NumericBaseType::I32)`, starting right after the 8000 -> -4000 erpm step at
  t = 1.0 s (sharpest transient in the run). Baked into `examples_mcu/mcu_qemu/src/motor_signal.rs`
  (and copied into `mcu_qemu_m4f`). Generated with a now-deleted scratch
  `examples/motor_demo/examples/scratch_dump_ia.rs` (private `Device` fields, so it had to live inside the
  `motor_demo` crate; regenerate the same way if the signal needs to change).
- `examples_mcu/mcu_qemu` (Cortex-M3, existing crate, target `thumbv7m-none-eabi`, qemu `lm3s6965evb`): three new
  bins `qemu-series-encode-cpu-{smallest,fixed-delta,fixed-dod}`, one per `CodecPolicy` (`src/bin/series_encode_cpu_*.rs`,
  shared body in `src/bench.rs`). Each loops n = 8/32/64, times `Series::to_ww_bytes` with semihosting `SYS_ELAPSED`
  (meant to be read under qemu `-icount shift=0`, where one virtual ns = one retired instruction), prints via
  semihosting stdout, exits.
- New crate `examples_mcu/mcu_qemu_m4f` (Cortex-M4F, target `thumbv7em-none-eabihf`, qemu `mps2-an386`): same
  three bins. Memory map (flash 0x00400000/4M, RAM 0x20000000/4M) confirmed against Zephyr's MPS2 board docs.
  `justfile`: added `check-examples-mcu-qemu-m4f`, wired into `check-examples-mcu`.
- `cargo check` and `cargo clippy` clean on both crates (only the pre-existing `empty_loop` warning shared with
  `bounded.rs`/`ww_gpio.rs`, not gated by CI for `examples_mcu`); `cargo fmt --check` clean on both.
- Flash measured (`llvm-size`, release): see the table already written into the design note's "Measured or to be
  measured" section (also in `CHANGELOG.md` under `[Unreleased]`). Finding: flash is within 70 B across all three
  codec sets on both cores — `CodecPolicy` is a runtime value read inside one shared generic encoder, not a
  per-codec monomorphization axis, so `SMALLEST`'s "try every codec" code stays linked into a `fixed`-only bin
  too. The design's "a fixed codec skips the trials" claim is about CPU time, not flash — said explicitly in the
  design note so piece 8's docs page doesn't repeat it as a flash saving.

## Blocked

- **`qemu-system-arm` is not installed** on this login (gpd-omarchy): `pacman -Ss qemu-system-arm` shows it in
  `extra/` but `which qemu-system-arm` fails. Installing it needs root (`sudo pacman -S qemu-system-arm`), which
  this session cannot do; `tpm ask` isn't wired up yet in this tpm version, so it went to the parent in words via
  `tpm work say`. Once installed: run each of the 6 bins (`cargo run --release --bin <name>`, picking up the
  `.cargo/config.toml` runner) and read the three `n=.. insns=.. bytes=..` lines per bin from the semihosting
  stdout; fold the instruction counts into the design note's table next to the flash numbers already there, and
  only then make the D4 (no_std default codec) recommendation — don't guess it from flash alone.
- Separately, the plan also asks to file an ask for Roman to plug a nucleo (DWT cycles on real hardware,
  nucleo_h743zi2 / nucleo_g0b1re already in `examples_mcu`) — said in the same `tpm work say`, still open, not
  blocking the qemu numbers.

## Next

1. Once `qemu-system-arm` lands: `cd examples_mcu/mcu_qemu && cargo run --release --bin qemu-series-encode-cpu-smallest`
   (and the other two M3 bins), same in `mcu_qemu_m4f` for the three M4F bins. Collect instructions per (n, policy, core).
2. Add the instruction-count columns to the table already in `prj/P2401-wireweaver/design/batched_streams.md`
   ("Measured or to be measured"), replace the "Blocked" note, and write the D4 recommendation the plan asks for.
3. `tpm work ready` once the table and recommendation are in.

# TEST-23 series-encode-cpu: handoff

Piece 4 of `prj/P2401-wireweaver/design/batched_streams.md` is done on this session's side: flash and
instruction-count table written into the design note's "Measured or to be measured" section and into
`CHANGELOG.md`, with a D4 recommendation. Parent session (batched-streams@omarchy-m1) is following up with
real DWT cycle counts on an STM32H725IG/M7 (J-Link, omarchy-m1) before D4 is final.

## What's in this commit

- `examples_mcu/mcu_qemu` (Cortex-M3, `lm3s6965evb`, existing crate) and `examples_mcu/mcu_qemu_m4f` (new crate,
  Cortex-M4F, `mps2-an386`): three bins each (`*-smallest`, `*-fixed-delta`, `*-fixed-dod`), one per
  `CodecPolicy`, encoding 64 real recorded `i_a` samples from `examples/motor_demo`'s simulator as
  `Series<Timed<Ampere>>` at n = 8/32/64.
- Semihosting `SYS_ELAPSED` does **not** give instruction counts (it is `get_clock()`, host wall-clock time,
  unrelated to `-icount`) — found out the hard way, see the design note and the doc comment at the top of
  `bench.rs` in both crates. Instructions are counted externally instead: `bench.rs`'s `mark_start`/`mark_end`
  (`#[inline(never)]`) bracket the measured `to_ww_bytes` call; a qemu run with
  `-accel tcg,one-insn-per-tb=on -d exec,nochain -D <tracefile>` logs one trace line per instruction, and the
  line distance between a `mark_start` and its matching `mark_end` (found by address via `nm`, minus the Thumb
  bit) is the instruction count, minus the same distance with nothing between the markers (calibration, the
  first of the four occurrences; the other three are n = 8, 32, 64 in that order).

## Reproducing / porting to the H725

```
cd examples_mcu/mcu_qemu   # or mcu_qemu_m4f
cargo build --release
nm target/<triple>/release/<bin> | grep mark      # mark_start / mark_end addresses, -1 for the Thumb bit
qemu-system-arm -cpu <cpu> -machine <machine> -nographic -semihosting-config enable=on,target=native \
  -accel tcg,one-insn-per-tb=on -d exec,nochain -D trace.log -kernel target/<triple>/release/<bin>
```
(`<cpu>`/`<machine>` from that crate's `.cargo/config.toml` runner line.) The design note's recipe has the exact
flags; the line-counting itself was a throwaway Python script (not committed) — straightforward to redo, or to
swap for real DWT `CYCCNT` reads around the same `mark_start`/`to_ww_bytes`/`mark_end` on the H725 (no qemu
trace needed there, DWT is real hardware).

## Numbers (also in the design note)

Flash is within 70 B across codec sets on both cores (`SMALLEST`'s trial code isn't eliminated by a fixed
policy — that's a CPU saving, not a flash one). Instructions: a fixed codec is 3.3x-3.9x cheaper than
`SMALLEST`, not the ~10x guessed; `fixed(delta,packed)` and `fixed(delta-of-delta,packed)` cost about the same.
At the full FOC rate (21 properties x 25 kHz) even a fixed codec needs ~1 billion instructions/second on
Cortex-M3 — no M3/M4/M4F/M7 part gets there, so full-rate all-properties batched observe needs a property
subset or a lower rate regardless of codec. D4 recommendation: `fixed(delta,packed)` as the no_std default.

## Next (parent's follow-up)

DWT cycle counts on the STM32H725IG (M7) via J-Link on omarchy-m1, then confirm or revise D4 with real cycles
next to these instruction counts (instructions are a proxy, ~1-1.5 cycles each from flash with wait states).

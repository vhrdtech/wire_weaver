# HANDOFF: series-encode-perf (P2401#17oct-2054)

Paused on 17 Oct 2026, evening: Roman stopped for the night while this session was still reading code. No code has changed yet, so this commit only holds this note.

## Task

Make `Series<T>` encoding fast and small on MCUs without changing any bytes (goldens and fuzz round trips must stay byte-identical). Three parts: (1) profile and fix the hot paths, (2) choose the codec at compile time so a fixed-codec firmware links only its codec, (3) a before/after table (insns per item at n=8/32/64, H725 cycles, flash per codec set) in `~/git/tpm/prj/P2401-wireweaver/design/batched_streams.md` "Measured" and in the docs. Target: several times fewer instructions, and under half the flash for a fixed codec.

## Done

- Read the encoder: `shrink_wrap/shrink_wrap/src/series/{mod.rs, group.rs, leaves.rs, series_type.rs, columns/{mod,numeric,run,float}.rs}`, the qemu harness `examples_mcu/mcu_qemu/src/{bench.rs, bin/series_encode_cpu_*.rs, bin/batch_cpu.rs}`, and `ww_si` `Timed`/`Quantity` (`Quantity { prefix: Prefix (enum, run), value: NumericValue (enum, series(no_store)) }`).
- Baseline numbers are already in the design note (TEST-23). M3 `fixed(delta,packed)`: 16 488 / 63 359 / 126 075 insns at n=8/32/64. Flash with ww_si+Series is ~98 KB against a 28.9 KB baseline.

## Suspects, from reading only (not measured yet)

- `numeric::encode`, packed coder: each block clones the whole `Steps` iterator and runs the predictor over the 8 items ahead, so the predictor runs about twice per item, and the item iterator under it is the nested `FieldItems` chain.
- `FieldItems` nesting (`Timed` → `Quantity` → `NumericValue` variant → leaf): every column walks all items through closures, and the `earlier` checks compute `series_tag`s of every field before it. Each leaf also calls `items.clone().next()` once to check for emptiness. With `NumericValue`'s variants, every variant's column walks all n items.
- `put_bits` on `BufWriter` goes through `write_un64`, which may write bit by bit (check `buf_writer.rs`). A word-at-a-time path would help every codec.
- `CodecPolicy` is a runtime value, so `smallest_numeric`, every trial and `float_bits` stay linked. Fix: a policy type parameter, e.g. a `SeriesPolicy` trait with a `SMALLEST` default type, with `Series::with_policy` kept as the runtime form; or cargo features. This changes the Rust API only, not the wire. Check `ww_stream::batch::DEFAULT_POLICY` and its callers.

## Next

1. Write a small Rust (or committed Python) instruction counter for the `-d exec,nochain` trace: the design note says the earlier counting script was thrown away. Also make a per-function histogram (address → symbol through `nm`) to see where the instructions go.
2. Fix the suspects, measuring each change. Run the golden tests (`series/columns/golden.rs`, `golden_signals.rs`) and the fuzz checks after each one.
3. Compile-time policy, then flash per codec set.
4. H725 on omarchy-m1 (J-Link, probe-rs over ssh, DWT CYCCNT). The board was not claimed yet. Release it when done.
5. Tables, CHANGELOG (`shrink_wrap`, root `[Unreleased]`, no version bump), `just test lint check-mcu check-examples-mcu`, then `git rm HANDOFF.md` and `tpm work ready`.

## Constraints from Roman

- Only proper fixes, and bytes stay the same.
- Keep the nextest limits tight: a slow test is a bug, never a reason to raise a limit.

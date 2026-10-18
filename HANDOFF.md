# HANDOFF: series-encode-perf (P2401#17oct-2054, FEATURES SW-63)

Updated 18 Oct 2026. Branch `s/17oct2026-series-encode-perf`, rebased onto master 0e0d02a (master has moved on since, to 41fc92f: run `tpm work rebase` before ready).

## Done (committed)

- Fast paths, same bytes. n=32 `Series<Timed<Ampere>>` on the M3: fixed(delta,packed) 63 359 → 10 922 instructions, SMALLEST 213 233 → 31 014. The M4F is similar.
  - packed blocks buffered;
  - `SeriesColumns::TAGGED` with `tag_changed`;
  - enum variants skipped when unseen;
  - u32/u64 `Word` predictor on left-aligned values;
  - one-pass `Sizes` for SMALLEST;
  - `BitWriter` keeps the open byte in a register;
  - `write_un32/64` rewritten;
  - push-style column writers.
- Flash. The derive emits a struct head only for evolvable structs. New `SeriesPolicy` trait, with `CodecPolicy` as the runtime policy and the `FixedCodecs` trait for compile-time ones, plus the `DeltaPacked` type. `Series<'i, T, P = CodecPolicy>`. `write_group` is generic over `P` (breaking, in the shrink_wrap 0.2.0 cycle). Encoder flash on the M3 went from 90.8 KB to 25.4 KB with a runtime policy and 15.0 KB with `DeltaPacked`; on the M4F from 68.5 KB to 17.6 / 9.8 KB.
- ww_stream: `Batcher/Producer/Consumer<T, N, P = DefaultPolicy>`. `DefaultPolicy` is `DeltaPacked` on no_std and `CodecPolicy` on std. `BatchSource::Policy`.
- `examples_mcu/qemu_icount`: an instruction counter with `--profile` and `--pcs=` options. `mcu_qemu(_m4f)` gained the `delta-packed` and `baseline` bins. The H725 bin `stm32h725ig-series-encode-cycles` was measured: n=32 DeltaPacked 25 241 cycles, before 104 027 (fixed). The board has been released.
- Docs: `docs/serdes/series.md` has a "CPU and flash on an MCU" section. FEATURES SW-63 is ✅. The features page entry is added and regenerated. CHANGELOGs are updated: shrink_wrap, ww_stream and root `[Unreleased]`, no version bump. Full tables are in `~/git/tpm/prj/P2401-wireweaver/design/batched_streams.md` under "After SW-63".
- `just test` (nextest, whole workspace) passed after the policy step, 1460 tests.

## Next (to finish)

1. `tpm work rebase` onto the current master.
2. `just lint` (clippy, `-D warnings`, also `--all-features`; the derive emits `false || tag_changed(..)`, so watch for clippy complaints in generated code), `just test`, `just check-mcu`, `just check-examples-mcu`. Also `cargo sort -w -g --check`, `typos` and rustdoc.
3. Fuzz briefly to confirm the round trips are unchanged: `just fuzz-shrink-wrap-series-columns` / `-series-type` (cargo +nightly fuzz), a minute each.
4. Ask the user before running `just build-docs` (it breaks a running `serve-docs`), then fix any new link warnings in `docs/serdes/series.md`.
5. `git rm HANDOFF.md`, commit, then `tpm work ready --note "SW-63: Series encode ~6x fewer insns, compile-time DeltaPacked 15 KB vs 91 KB flash, H725 cycles in docs"`.

## Gotchas

- `cargo fmt` in examples_mcu crates also reformats unrelated bins (ww_gpio, blinky, uart...): this is harmless formatting churn and is already committed.
- The kitchen_round_trips test took about 2 s in debug, near the 3 s limit. Check it under load. If it is slow, make the test cheaper; don't raise the limit.
- The fixed-delta qemu bin uses `FloatCodec::Plain`, and `DeltaPacked` uses Xor. This doesn't matter here: the signal has no floats.

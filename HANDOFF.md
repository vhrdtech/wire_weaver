# Handoff: ww-io-traits (STD-13 io-trait-family, task P2401#14oct-1548)

Done:
- Round 1 design reviewed by Roman (14 Oct, full text: ~/.local/state/tpm/say/1791988998-user-0.txt).
- Round 2/3: `docs/std_library/io_family.md` answers every point, revised design (split traits, ww_si values and
  timestamps, no Ticks, no u32 pin masks, ww_logic_analyzer + ww_signal_generator, no read_many).
- `ww_i2c` rewritten, `ww_adc` new: compile std/no_std, clippy/rustdoc clean, snapshots saved (--force, never
  published). `ww_si`: owned/defmt/Debug derives on every quantity.
- Review renders (not committed): target/review/{io_family,ww_i2c,ww_adc}.html (pandoc).

Round 2 answers (14 Oct, ~/.local/state/tpm/say/1791993272-user-0.txt) applied as round 3: seq/lost out (ww-stream-reliability),
plain errors without hand-typed future_version (ww-errors-plan), I2cBus merged into I2c, 10-bit I2C addresses
(I2cAddress, measured +1 byte), AdcStream start/stop via sideband Open/Close, no timestamp Options.

Next (after round 3 approval):
1. ww_si / ww_numeric improvements (time semantics + Unwrapper, constructors, UVlq32/64 in NumericValue).
2. Skeletons of the other crates here; then claim STD ids, file sonnet fill-ins, spawn --from-here.
3. cnt in examples_mcu (runner = "cnt run --chip ..."), separate task.

Decisions/gotchas:
- None of ww_gpio/uart/i2c/spi/si is on crates.io: versions stay 0.1.0, snapshots re-saved with --force.
- `#[ww_trait]` expands to nothing, so rustdoc never shows trait methods: refer to traits in backticks, not
  [links] (rustdoc -D warnings fails). Framework idea filed in the doc.
- The API parser needs types from a crate's direct dependencies (ww_numeric::NumericValue via ww_si re-export
  failed: "Module ww_numeric not found"): depend on ww_numeric directly.
- SW-47 (ww-errors-plan, 54ed35da, not on master yet): Unsized enums get Unknown(u32) automatically; mark
  complete-by-nature ones `no_future_version` when it lands (I2cOp, I2cReadKind? SampleFormat stays open).
- SW-16 is on master: `#[future_version] Unknown(u32)` as last variant, opt-in.
- Global-trait client (ww-cross-version): DynClient::traits_only + global_attachment; trait must not sit in an array.

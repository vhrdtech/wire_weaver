# Handoff: ww-io-traits (STD-13 io-trait-family, task P2401#14oct-1548)

## State (14 Oct, commit 8433673f and later)

- Design **approved by Roman** after 3 rounds: `docs/std_library/io_family.md` (answers per round, per-crate
  kept/changed/added tables). Round texts: ~/.local/state/tpm/say/1791988998-user-0.txt (r1),
  1791993272-user-0.txt (r2); r3 answer: 10-bit I2C via SW-48 at no cost, `I2cWatch` trait (IF-6), build it all.
- Skeletons committed, all compile std + no_std, clippy/rustdoc `-D warnings` clean, snapshots saved (`--force`,
  none of these crates was ever published, versions stay 0.1.0), `wire_weaver_core` snapshot test passes:
  `ww_gpio` (Pin::capabilities, errors), `ww_uart` (credit tx, UartMonitor), `ww_i2c` (I2c, I2cWatch,
  I2cMonitor, `I2cAddress { Seven(U7), Ten(U10) }`), `ww_spi`, `ww_adc`, `ww_pwm`, `ww_onewire`, `ww_timebase`.
  `ww_si`: owned/defmt/Debug derives on every quantity. CHANGELOGs under Unreleased, FEATURES STD-13, STD-17..23.
- Review renders (not committed): target/review/{io_family,ww_i2c,ww_adc}.html (pandoc), may be stale.

## Ids claimed (FEATURES)

SW-48 sub-byte fields (task P2401#14oct-1834, child **ww-subbyte-fields** on omarchy-m1, running, from master),
STD-17 ww_timebase, STD-18 ww_adc, STD-19 ww_pwm, STD-20 ww_onewire, STD-21 ww_logic_analyzer, STD-22
ww_signal_generator, STD-23 ww_si/ww_numeric extension, EX-14 cnt in examples_mcu. (Parent api3-design holds
STD-14/15, not ours.)

## Children (spawned 14 Oct, step 1 done in a13a8cea: tests/io_<x>{,_api} pairs, docs stubs, nav)

All `--from-here` off a13a8cea on gpd-omarchy unless noted (`--from-here` only works on this PC):
io-gpio (STD-13, P2401#14oct-1846), io-uart (-1846b), io-i2c (-1846c), io-spi (-1846d), io-adc (STD-18, -1846e),
io-pwm (STD-19, -1846f), io-onewire (STD-20, -1846g), io-timebase (STD-17, -1846h): sonnet fill-ins.
si-numeric (STD-23, -1846i, opus), logic-analyzer (STD-21/22, -1846j, opus: design doc to us first, we forward
it to Roman). cnt-examples (EX-14, -1846k, sonnet) on omarchy-m1 from master.
Prompt common part: $CLAUDE_JOB_DIR/tmp/common.txt (not durable; it is in each child's first prompt).

## Next steps

1. Wait (parked). As children report ready: review their branch, merge into ours (`git merge s/14oct2026-io-<x>`),
   resolve conflicts (FEATURES.md, wire_weaver_snapshots, Cargo.lock likely), `just test`, `just lint`.
2. Once io-gpio, io-uart, io-i2c, io-spi are merged here: tell parent api3-design (it starts WW-4 / STD-14 hl crates from our branch).
3. Forward logic-analyzer's design doc to Roman for review.
4. When all are in: `just build-docs` (ask Roman first, serve-docs), streams.md conventions check, review renders,
   tell Roman; `git rm HANDOFF.md`, `tpm work ready`.

## Coordination

- **ww-errors-plan**: errors design approved (`Common(CommonError)` first, `#[errors(..)]` per method, `#[optional]`,
  SW-47 automatic Unknown). Its stdlib migration P2401#14oct-1805g covers **every** IO crate (agreed); it tells us when
  ww_error / #[errors] are usable. Until then error enums stay plain; every method has an `# Errors` rustdoc section.
- **ww-stream-reliability**: seq / lost / backfill / per-chunk timestamps / Open with typed config; not decided,
  asked not to change traits for it yet. Stream items carry no seq/lost.
- **ww-rustdoc-traits** (DOC-6): rustdoc for #[ww_trait]; our crates name traits in backticks.
- **ww-subbyte-fields** (SW-48): U7/U10 already exist in `shrink_wrap::un` on master (derive works); using them
  directly as ww_trait method args was fixed on its branch (we only use them inside the enum, which works).

## Gotchas

- `#[ww_trait]` expands to nothing: rustdoc never shows the traits; `[TraitName]` links fail `-D warnings`.
- The API parser resolves types only through a crate's direct dependencies (depend on ww_numeric directly for
  NumericValue).
- `cargo sort -w -g` also rewrites `ww_stdlib/ww_firmware_info/Cargo.toml` (unsorted on master); don't commit that.
- Shell heredoc commands writing many files at once got interrupted once; write files one by one.

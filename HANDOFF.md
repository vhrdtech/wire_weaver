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

## Next steps

1. **Pre-create shared touch points** so 8 parallel fill-ins don't conflict in Cargo.toml / Cargo.lock /
   zensical.toml: test crate pairs `tests/io_<x>` + `tests/io_<x>_api` for x in gpio, uart, i2c, spi, adc, pwm,
   onewire, timebase (layout like `tests/credit_sink{,_api}`; api crate: `#[ww_api_root] pub trait IoXTest {
   ww_impl!(x: ww_x::Trait); ... }` like `examples/all_gpio_api`; test crate deps: io_x_api, tests_common, tokio,
   tracing-subscriber, wire_weaver, wire_weaver_client, ww_x; `publish = false`; root `[workspace.dependencies]`
   entries for every `io_*_api`), empty `#[cfg(test)] mod tests {}`; docs page stubs
   `docs/std_library/ww_{uart,i2c,spi,adc,pwm,onewire,timebase}.md` + nav entries in `zensical.toml` (ww_gpio.md
   exists). `cargo check --workspace`, commit.
2. **File tasks** (`tpm add P2401 '... model:X'`, then `tpm work claim <ID> --task REF`) and **spawn**
   (`tpm work spawn SLUG wire_weaver --task REF --from-here --on omarchy-m1` mostly; omarchy-m1 had room for 19):
   - 8 fill-ins, **sonnet**, `--from-here`: per crate full rustdoc, host helpers (std), the test in its
     `tests/io_<x>` pair with a mock server through `tests_common::start_device` (one in-process test per trait at
     least), its docs page, snapshot (`ww api save ww_stdlib/ww_x --force`, copy to wire_weaver_snapshots), CHANGELOG,
     its FEATURES line; touch nothing else shared. Extras: ww_gpio moves `ww_gpio_hl` + `examples/all_gpio` +
     `examples_mcu` users; ww_uart moves `examples/uart`; ww_i2c tests I2cWatch semantics (Always / Changed mask /
     Matches start+end, errors once, host calls spliced) and checks `I2cAddress::seven(0x50)` is 1 byte on the wire.
     Ids: gpio/uart/i2c/spi under STD-13, adc STD-18, pwm STD-19, onewire STD-20, timebase STD-17.
   - STD-23 ww_si/ww_numeric extension, **opus**, `--from-here`: io_family.md "ww_si and ww_numeric improvements"
     items 1-3, 5 (features per quantity group, Second as timestamp + Unwrapper, constructors, UVlq32/64).
   - STD-21 + STD-22 logic analyzer and signal generator **design pass, opus**: doc for Roman's review first
     (encodings Raw / Transitions with SW-31 DeltaOfDelta, triggers, capture vs stream), then skeletons.
   - EX-14 cnt in examples_mcu, **sonnet**, from master: counters in MCU examples / test firmwares,
     `runner = "cnt run --chip ..."` per board `.cargo/config.toml` (see usb_io_fw), `just check-examples-mcu`.
3. Tell the parent (`tpm work say parent`) the plan in a few lines; park while children work; integrate/review their
   branches as they report ready.

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

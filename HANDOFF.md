# Handoff: ww-errors-plan

Design of errors and the trait surface for all APIs (Roman's IO family review, 14 Oct 2026).

## Done
- `docs/api/errors.md`: design for review (per-method errors, common errors, SW-47, optional items), plan table,
  decisions for Roman. Downstream claims checked in a throwaway prototype (plain Rust, anyhow + thiserror).
- FEATURES: SW-47 planned, API-16 rewritten as optional items, new CLT-24, STD-16, API-32, API-33 (idea), DER-12.

- SW-47 built (derive adds `Unknown(u32)` to Unsized enums; `no_future_version`, `future_version = Name`), tests,
  docs (derive.md, rules.md), CHANGELOGs; FEATURES SW-47 done, INT-8 idea. just check / test / lint pass.

## Next
- Roman approved all (14 Oct): RpcResult/GetResult/SetResult unchanged, CallError has Device / Unimplemented / Client.
- CLT-24 merged (54e3c912). Children from ebfd76d1, this session integrates (merges their branches here, runs just check/test/lint):
  ww-call-error (CLT-24), ww-display-docs (DER-12), ww-common-error (STD-16), ww-optional-items (API-16).
- After STD-16 is merged: spawn API-32 (P2401#14oct-1805e) from here; after all: migration (P2401#14oct-1805g)
  together with ww-io-traits, covering every IO crate whose skeleton exists by then (ww_gpio, ww_uart, ww_i2c,
  ww_spi, ww_adc, ww_pwm, ww_onewire, ww_timebase; later ww_logic_analyzer, ww_signal_generator) plus ww_dfu,
  ww_can_bus; the '# Errors' rustdoc sections are the input for #[errors(..)].

## Gotchas
- ww_i2c's `I2cReadKind` gets `Unknown` too; its CHANGELOG lives on the ww-io-traits branch, so no entry here.
- Stdlib enums complete by nature (`ww_uart` `StopBits`, `Parity`) got `Unknown` as well: the migration task should
  mark them `no_future_version`.
- Narrow per-method types: `?` from `CallError<Narrow>` into a thiserror `#[from] CallError<Wide>` doesn't compile;
  a generic `impl<E> From<CallError<E>> for X where Wide: From<E>` does.
- ww-io-traits keeps its error enums plain until this design lands (message of 14 Oct).

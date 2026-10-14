# Handoff: ww-errors-plan

Design of errors and the trait surface for all APIs (Roman's IO family review, 14 Oct 2026).

## Done
- `docs/api/errors.md`: design for review (per-method errors, common errors, SW-47, optional items), plan table,
  decisions for Roman. Downstream claims checked in a throwaway prototype (plain Rust, anyhow + thiserror).
- FEATURES: SW-47 planned, API-16 rewritten as optional items, new CLT-24, STD-16, API-32, API-33 (idea), DER-12.

- SW-47 built (derive adds `Unknown(u32)` to Unsized enums; `no_future_version`, `future_version = Name`), tests,
  docs (derive.md, rules.md), CHANGELOGs; FEATURES SW-47 done, INT-8 idea. just check / test / lint pass.

## Next
- All five pieces are in, rebased onto master 3539cdf (DOC-6) as plain commits (no merges: tpm land rebases): SW-47, CLT-24
  (+ acbb7068 port of master's CLT-10 code), DER-12, API-16, STD-16. just check + 702 tests pass; just lint stopped at
  cargo doc on a full /mnt/data (reported to parent).
- Display convention settled: messages without the trailing period everywhere; CommonError stays hand-written
  (empty parts left out).
- API-32 running in child ww-error-lists (from this branch on master 3539cdf). Then the migration (1805g) covering
  every IO crate with ww-io-traits.
- Rebase with plain `git rebase origin/master` before ready (local master lags origin).

## Gotchas
- ww_i2c's `I2cReadKind` gets `Unknown` too; its CHANGELOG lives on the ww-io-traits branch, so no entry here.
- Stdlib enums complete by nature (`ww_uart` `StopBits`, `Parity`) got `Unknown` as well: the migration task should
  mark them `no_future_version`.
- Narrow per-method types: `?` from `CallError<Narrow>` into a thiserror `#[from] CallError<Wide>` doesn't compile;
  a generic `impl<E> From<CallError<E>> for X where Wide: From<E>` does.
- ww-io-traits keeps its error enums plain until this design lands (message of 14 Oct).

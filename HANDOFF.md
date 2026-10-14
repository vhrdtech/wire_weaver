# Handoff: ww-errors-plan

Design of errors and the trait surface for all APIs (Roman's IO family review, 14 Oct 2026).

## Done
- `docs/api/errors.md`: design for review (per-method errors, common errors, SW-47, optional items), plan table,
  decisions for Roman. Downstream claims checked in a throwaway prototype (plain Rust, anyhow + thiserror).
- FEATURES: SW-47 planned, API-16 rewritten as optional items, new CLT-24, STD-16, API-32, API-33 (idea), DER-12.

## Next
- Roman's answers to "Decisions for Roman" in the doc; adjust the doc.
- SW-47 implemented in this session; the other pieces as child sessions (tasks in P2401).

## Gotchas
- Narrow per-method types: `?` from `CallError<Narrow>` into a thiserror `#[from] CallError<Wide>` doesn't compile;
  a generic `impl<E> From<CallError<E>> for X where Wide: From<E>` does.
- ww-io-traits keeps its error enums plain until this design lands (message of 14 Oct).

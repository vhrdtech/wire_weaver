# Handoff: sw-size-2 (shrink_wrap code size, items 2-4 of docs/serdes/code_size.md "What is left")

## Done
- A (SW-38): latched reads of `U1`..`U64` / `I2`..`I64` in derived types whose every field is latched.
  `bits_de` 886 -> 846 B RV32IC, every other bench case unchanged. Nibble/u64/varints NOT latched (cost for
  one field, numbers in code_size.md). Tests: shrink_wrap/tests/latched_reads.rs, fuzz `shrink-wrap-latched`.

- B (SW-39): byte-wise memcpy as an option of the bench (`--fw-features byte_memcpy`), documented as advice for
  very small targets with its downsides (user's wish). -324 B RV32IC per image that moves a value.

## Next (user, 12 Oct 21:01: the two fixes before C, each its own commit + SW id + CHANGELOG Fixed)
- Fix 1: bit reads must not read into the reversed-length nibble (read_un*, read_bool, latched, writer mirror).
- Fix 2: mixed latched/non-latched types return the FIRST error (+124 B busgen accepted, find the cheapest form).
- C: finish_for in ww_codegen! output (wire_weaver_core) and ww_client_server.
- End: update "What is left", git rm HANDOFF.md, tpm work ready.

## Decisions / gotchas
- Baseline for each step: a detached worktree of HEAD at /tmp/sw-size-2-base (remove with
  `git worktree remove /tmp/sw-size-2-base` when done), bench files copied in.
- Pre-existing, not fixed (reported to parent): (1) mixed types (latched fields + list/nested) can return a later
  error instead of the first (since SW-34 step 3); fix measured +124 B busgen. (2) `read_un32`/`read_un64` read
  into the nibble a reversed length took from the last byte (`read_un8(6)` with 4 bits left gives Ok).
- Don't touch shrink_wrap_derive/src/transform/transform_ty.rs (session sw-derive-type-group).

# Handoff: ww-si-cfg-signature (P2401#16oct-1121)

## Done

- Root cause of `type ww_si::Volt ... has a different definition on each side` found, it is NOT cfg features:
  introspection is made by `ww_codegen!` re-parsing source text, every `#[cfg]` variant is listed.
  - omarchy-m1 device bundles: fw from 15 Oct 22:36 stores Volt signature `049a2a94f6d68bb5`, 16 Oct 08:01 builds
    `0af8195a2e0069fe` (= current master).
  - Building `be435274^` and computing Volt's signature from its snapshot gives exactly `049a2a94f6d68bb5`.
  - be435274 (TOOL-11) added `ItemStruct.sparse` / `Field.slot` to ww_self; a signature hashes the serialized
    ww_self canonical bundle, so every struct signature moved (its commit message says so).
- `tests/cfg_variants` (+ `cfg_variants_api`, `cfg_variants_narrow_api`, same source, narrow built without
  features): connect, exchange shared variants and a skipped `ww_si::Volt`, introspection lists every variant,
  lacking variant answered with `ArgsDesFailed`. Passes with no code change.
- derive.md `#[cfg]` section: introspection/signatures list every variant.

## Next (waiting for Roman's choice via parent)

- (c) signature over a frozen encoding: ww_self model fields added later (sparse, slot, future) only fed when not
  default, so additions never move signatures; accept the legacy (pre-TOOL-11) signature too, or reflash.
- (a) codegen skips only definitions of published crate versions; (b) clearer error + structural fallback.
- Then CHANGELOG [Unreleased], FEATURES item, just test / lint / check, `git rm HANDOFF.md`, tpm work ready.

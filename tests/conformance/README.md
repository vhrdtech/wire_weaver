# conformance

The wire-format conformance matrix (TEST-23, `docs/serdes/conformance.md`): one corpus of cases (`corpus.json`) sent through every consumer of the wire format — the Rust derive's owned and borrowed types, the `ww_self` dynamic client, Python (`tests/conformance_py`) and C.

A case a consumer can't express yet carries a gap mark with the reason; the tests fail when a mark is missing or no longer needed.

`just conformance-update` fills in golden bytes for new cases and writes the report at `docs/serdes/conformance_matrix.md`.

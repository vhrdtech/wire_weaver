# conformance_api

Types of the wire-format conformance matrix (TEST-23, `docs/serdes/conformance.md`): one property of the `Conformance` trait per case type, named so every consumer (the Rust case table, the dynamic client, Python) can find it from `tests/conformance/corpus.json`.

Types without a sw serdes form, or not yet in the API model, are defined here but left out of the trait; the corpus marks them for the consumers that can't read them yet.

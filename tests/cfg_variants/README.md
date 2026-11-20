# cfg_variants

Tests `#[cfg]` enum variants (DER-8) across builds: a device built from `cfg_variants_narrow_api` (without the `wide` and `float` features, as firmware would be) connects to a host built from `cfg_variants_api` with every variant on.

Checks that the compatibility check passes, the variants both sides have are exchanged, and introspection lists every variant regardless of which features are on.

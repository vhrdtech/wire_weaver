# cfg_variants_narrow_api

The firmware-side build of the `#[cfg]` enum variants API (DER-8): the same source as `cfg_variants_api`, built without the `wide` and `float` features.

Used by `tests/cfg_variants` to connect a host built with every variant to a narrower device.

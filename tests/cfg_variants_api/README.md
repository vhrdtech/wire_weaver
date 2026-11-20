# cfg_variants_api

API with `#[cfg]` enum variants (DER-8), built with every variant (`wide`, `float`) on, as a host would be.

`tests/cfg_variants_narrow_api` is the same source built without them, as firmware would be; `tests/cfg_variants` connects the two.

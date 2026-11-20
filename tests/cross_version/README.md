# cross_version

Tests cross-version calls (API-20, CLT-10): a device on an old API version is rebooted by a host built against a wire-incompatible new version, through a global trait path a plain generated client can't reach.

Covers both `GlobalCompact` (by the trait's `ww_global` id) and the `GlobalFull` fallback a device without that id, or without introspection, needs.

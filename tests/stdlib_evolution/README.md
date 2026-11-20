# stdlib_evolution

Checks the framework crates with API snapshots (`ww_link`, `ww_client_server`, `ww_timebase`, `ww_tap`) against
their own `api_snapshots/`, one test per crate, from this crate rather than each crate's own `tests/evolution.rs`
(FEATURES.md TOOL-31/EVO-71: see docs/evolution/checker_tool.md).

Kept separate because the `wire_weaver_evolution` dev-dependency turns on wide number features in shared deps
through cargo's feature unification, which would break narrow feature-set tests sitting on the crate itself.

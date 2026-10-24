# evo-current handoff

Done (4222d39b): EVO-12 `Check::require_current(true)`, `Check::save(force)`, `WW_API_SAVE=1` in `Check::assert()`, `ww api save` on `Check::save` (refreshes the current version's snapshot only), `ww api check --require-current`, docs/evolution/checker_tool.md (lockfile flow, released-snapshot hook), FEATURES, CHANGELOGs, blinky_api uses require_current. Parent g0-gen-suite told the final API.

Passed: `cargo nextest run -p wire_weaver_evolution -p blinky_api` (34/34), features page test, manual `ww api save` / `ww api check --require-current` runs.

Next: `just test --changed` (was killed: the PC ran low on memory, not a test failure) and `just lint`, then `git rm HANDOFF.md` and `tpm work ready`.

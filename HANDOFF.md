# evo-current handoff

Done (4222d39b): EVO-12 `Check::require_current(true)`, `Check::save(force)`, `WW_API_SAVE=1` in `Check::assert()`, `ww api save` on `Check::save` (refreshes the current version's snapshot only), `ww api check --require-current`, docs/evolution/checker_tool.md (lockfile flow, released-snapshot hook), FEATURES, CHANGELOGs, blinky_api uses require_current. Parent g0-gen-suite (omarchy-m1) has the final API.

Passed: `cargo nextest run -p wire_weaver_evolution -p blinky_api` (34/34), features page test, manual `ww api save` / `ww api check --require-current` runs.

Filed: P2401#24oct-1902 (guard released snapshots in wire_weaver's own `just save-snapshots`), as g0-gen-suite asked; don't build it here.

Next: `CARGO_BUILD_JOBS=6 NEXTEST_TEST_THREADS=6 just test --changed`, then `just lint`, then `git rm HANDOFF.md`, commit, `tpm work ready --note "..."`.

Gotchas: gpd-omarchy is short on RAM. `just test --changed` was killed twice for low memory (not a test failure), and a third run with CARGO_BUILD_JOBS=6 was still compiling after 60 min when the conversation ended. Run it in the foreground or watch it with Monitor: a background shell gets reaped while the session is idle. If it is killed again, park and tell g0-template-suite (it asks Roman), as g0-gen-suite asked.

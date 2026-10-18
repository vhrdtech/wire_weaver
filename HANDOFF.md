# motor-demo-move handoff

Done:
- motor_sim (branch s/18oct2026-motor-demo-move, worktree ../motor_sim): workspace (motor_sim/, motor_demo/, motor_demo_api/); motor_demo history imported with `git format-patch --relative=examples` from wire_weaver 0e0d02aa + `git am`; wire_weaver deps by path ../wire_weaver; justfile test/lint/install/deps-pull/deploy; .config/nextest.toml; CI checks out wire_weaver as sibling; README, AGENTS, FEATURES (DEMO-1 done, DEMO-2 GUI idea), CHANGELOG. just test + just lint green, `cargo run -p motor_demo -- observe --compare` works.
- wire_weaver: examples/motor_demo{,_api} removed, workspace deps and nextest override gone; tests/observe gained levels_have_their_own_budgets + the one-update-vs-update-each size check in periodic_rates; docs point at motor_sim; FEATURES EX-11, features page, AGENTS, CHANGELOG. just lint green, just test green except ww_settings power_cut g0_every_step_4_pages timing out at 15 s under PC load (12 s alone, unrelated).

Next:
- wait for ww-deps-trim to land, `git rebase master`, resolve Cargo.toml/Cargo.lock plainly (drop motor_demo_api/motor_sim lines), re-run just test/lint here
- re-run motor_sim just test/lint against the rebased wire_weaver (ww-time-exact may land first and change ww_si time APIs the demo uses)
- git rm HANDOFF.md, tpm work ready --note, tpm work say parent

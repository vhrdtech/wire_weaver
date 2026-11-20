# wire_line_fixture_api

Stands in for a real `ww_stdlib` crate (R7) in `wire_weaver_client`'s tests: a sub-trait whose `#[since]` names
a revision of this crate's own wire line (w2.0), not its API root's (FEATURES.md EVO-26:
wire-versions-of-shared-crates).

Used instead of a path dependency on `ww_stdlib/ww_board_info` in the compat/global_paths tests.

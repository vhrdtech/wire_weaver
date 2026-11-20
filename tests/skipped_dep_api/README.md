# skipped_dep_api

Stands in for a real `ww_stdlib` crate (R7) in `wire_weaver_core`'s snapshot and definition cache tests: an
API using these traits leaves their definitions out against this crate's own snapshot, in place of a path
dependency on `ww_uart`/`ww_gpio`.

Used by `tests/skipped_root_api` and the snapshot tests in `wire_weaver_core`.

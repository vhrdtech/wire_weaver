# pins_dep

Stand-in dependency for `wire_weaver_evolution`'s own `pins::refresh` tests (EVO-70, `tests/check.rs`): a real
workspace member, so its definitions resolve the way a real dependency's do, framework types only (`ww_api`).

Its `src/lib.rs` is copied and edited in place by those tests to drive a changed dependency; never edited here
by hand.

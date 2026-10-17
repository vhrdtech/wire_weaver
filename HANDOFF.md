# Handoff: CLT-31 quantities in clients (session ww-si-clients)

Done (committed): ww_self `instance_of` on concrete definitions; `wire_weaver_client::quantity` (DynQuantity, DynUnit, unit_of, to_value); `ww introspect` / `ww observe` typed formatting; Python `ww.Quantity`; tests `tests/quantities`, `wire_weaver_py/tests/test_offline.py::test_quantities`; changelogs (ww_self, ww_si, client, cli, py); FEATURES CLT-31, features page, `docs/std_library/ww_si.md#in-ww-python-and-dynamic-clients`.

Open: `instance_of`'s None flag changes the serialized bytes, so every signature / API hash / snapshot .ron changes once (+33 B over all snapshots). 3 tests fail until `just save-snapshots`: wire_weaver_core snapshot::tests::{known_definitions_are_left_out_and_put_back, snapshots_are_up_to_date}, wire_weaver_snapshots tests::snapshots_can_be_inlined. Waiting for the parent: A re-save (recommended), B host-side table, C derive host-only field.

Decisions: no `ww property get` exists, so introspect + observe got the symbols; types stay named by their definitions (`Timed<Quantity<Ampere>>`, D2 a; alias names are piece 7); Python ints write as I32/I64, floats as F64 at Prefix::Unit. `just build-docs` not run (it breaks a running `just serve-docs`); anchor checked by hand.

# lean_server

A server of `flash_budget_api::BaseOnly` built the way firmware builds one (SRV-60): `ww_client_server` without
`std`, so without its `multi` and `streams` features.

`tests/left_out.rs` sends it the multi, credit sink and stream sideband requests of the conformance corpus and
checks that it refuses them as not supported, instead of failing to read them. Its handlers are the stubs of
`ww api scaffold tests/flash_budget_api --root BaseOnly`.

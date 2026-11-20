# standalone

Host-side tests driving `standalone_api`'s generated server and client, where the API's types come from
`standalone_units`, a plain data crate with no `wire_weaver` dependency of its own (API-60:
types-from-crates-that-arent-api-crates).

Checks that generated code names those types through `standalone_api`'s re-export, with no direct dependency on
`standalone_units`.

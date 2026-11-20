# local_trait

> One API used through the generated client and by a plain local struct, with one call site

Depends on `local_trait_api` (the `Counter` trait), `wire_weaver`, `wire_weaver_client` and `ww_client_server`.
`exercise` is written once against the `Counter` trait and runs unchanged against `LocalCounter` (a plain struct
called directly, no server, no serialization) and against the generated `CounterClient` talking to a real or
simulated device. `CounterServer` shows the matching server-side shape: handlers that forward to the same local
implementation, so the logic is written once and served remotely only when needed.

See the [main repo](https://github.com/vhrdtech/wire_weaver) for the full picture and
[documentation](https://vhrdtech.github.io/wire_weaver) for the step-by-step guide.

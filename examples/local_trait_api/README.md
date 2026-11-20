# local_trait_api

> API of the local_trait example: a counter used by a client and by a local struct

An API-only crate (`no_std` by default, `std`/`local` features for the in-process use), depending on `wire_weaver`
and `ww_api`. Declares the `CounterApi` trait (`add`, `split`, `snapshot`, and a stream of totals) used two ways by
the `local_trait` example: through a device or service with the generated client, and in the same process by a
plain struct implementing the trait directly.

See the [main repo](https://github.com/vhrdtech/wire_weaver) for the full picture and
[documentation](https://vhrdtech.github.io/wire_weaver) for how to write an API crate.

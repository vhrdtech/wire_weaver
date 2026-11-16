# ww_api

> What a WireWeaver API crate needs to describe an API, without the runtime

An API crate (`my_device_api`, every `ww_stdlib` crate) depends on this crate: `use ww_api::prelude::*;` brings
`#[ww_trait]`, `#[ww_api_root]`, `ww_impl!`, `#[derive_shrink_wrap]`, the `shrink_wrap` prelude, `CommonError`
and `ww_version`. The runtime (`Context`, `EventOut`, the server and client traits) lives in `wire_weaver`, which
server and client crates take; an API crate never needs it.

By default, this crate has the `std` feature enabled; on `no_std`, add `default-features = false`.

Please see the [main repo](https://github.com/vhrdtech/wire_weaver)
or [documentation](https://vhrdtech.github.io/wire_weaver) for more information.

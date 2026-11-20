# ww_hub_api

> API of the ww hub: the per-user process that holds devices and serves every program (SRV-13)

An API-only crate (`no_std` by default, `std` feature), depending on `ww_api` and `ww_si`. Declares the `Hub`
trait (SRV-13, SRV-28): devices (USB, RTT (real-time transfer), ...) and info served like any device serves its
API, so introspection,
versioning, the dynamic client and Python work on it unchanged. The crate's version is the API's identity:
`Hub::info` and `Hub::handover` are frozen, so a client of any version can find out what hub runs and have an
older one replaced (SRV-30); everything else evolves by the usual rules. Implemented by `ww_hub`.

See the [main repo](https://github.com/vhrdtech/wire_weaver) for the full picture and
[documentation](https://vhrdtech.github.io/wire_weaver) for how to write an API crate.

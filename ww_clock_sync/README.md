# ww_clock_sync

> Host clock sync for WireWeaver devices: fits each device's clock against the host's and steers it onto UTC
> (Coordinated Universal Time)

Host-side clock synchronisation (STD-49) for devices serving `ww_timebase::Clock`: every device ends up on the
host's UTC. The real code lives in `wire_weaver_client::clock_sync` (its `clock_sync` feature, on by default),
where every direct connection steers its device by itself (CLT-46); this crate re-exports it, so programs that
steer with `ClockSync` directly keep working.

See the [main repo](https://github.com/vhrdtech/wire_weaver) for the full picture and
[documentation](https://vhrdtech.github.io/wire_weaver) for the step-by-step guide.

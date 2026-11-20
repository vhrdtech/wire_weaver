# ww_timebase

> Device clock and time synchronisation data types and API

Every device has one free-running clock that timestamps everything it captures, as seconds since the clock
started. `Clock` lets a host read it in full and synchronise several devices: one is the master on a sync line,
the others lock to it and publish `time_map` points mapping their local time to the shared one. Shared time is
one timescale, UTC (Coordinated Universal Time, STD-49), each source an estimate with an error bound; a device
follows its best source
(`SyncSource`) by default a locked sync line before host steering (`steer`) before holdover before running free,
and `set_sources` replaces that ranking. Switching sources slews the shared clock instead of stepping it.

See the [main repo](https://github.com/vhrdtech/wire_weaver) for the full picture and
[documentation](https://vhrdtech.github.io/wire_weaver) for how to write an API crate.

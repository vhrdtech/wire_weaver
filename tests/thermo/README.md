# thermo

A host driver for every wire line of `thermo_api` (FEATURES.md EVO-28/EVO-42: a-host-for-several-wire-lines,
the-multi-line-bench-on-one-crate): one generated client per line, behind one facade (`Thermo`) built with
`wire_weaver_client::facade!`.

The connection speaks every line; link setup picks the client matching the device's reported line, and
`Thermo::reconnect()` picks again after a lost link or a DFU onto another line.

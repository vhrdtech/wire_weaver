# ww_framer

> Packs many small WireWeaver messages into one packet or splits a big one across several

`Tx` packs small messages into one packet on the sending side; `FramedRx` (or `owned::FramedRxOwned` with the
`std` feature) reassembles pieces back into messages on the receiving side. `no_std` and allocation-free by
default; `std` adds owned, allocating variants. `large`/`very_large` raise the maximum message size the framer
accepts beyond the default.

See the [main repo](https://github.com/vhrdtech/wire_weaver) for the full picture and
[documentation](https://vhrdtech.github.io/wire_weaver) for the step-by-step guide.

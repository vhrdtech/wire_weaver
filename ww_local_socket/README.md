# ww_local_socket

> WireWeaver over local sockets: user-only Unix sockets and named pipes, the hub preamble, message framing

How a program reaches a device served by another process on the same PC, most of all the ww hub (SRV-13), which
holds the devices and serves every program of the user (TR-16). `runtime_dir` is the per-user directory sockets
and lock files live in; `listen`/`connect` open a Unix domain socket on Linux and macOS or a named pipe on
Windows (through `interprocess`), reachable by the same user only, and each end checks who is on the other side.
`Preamble`/`Answer` are the first bytes of every connection, picking the hub's own API or one of its devices;
`HandoverRequest`/`HandoverAnswer` ask a direct holder of a device to hand it to the hub (CLT-43); both are
frozen. `MessageReader`/`MessageWriter` carry `ww_link` messages after the preamble.

See the [main repo](https://github.com/vhrdtech/wire_weaver) for the full picture and
[documentation](https://vhrdtech.github.io/wire_weaver) for the step-by-step guide.

# ww_local_socket

> WireWeaver over local sockets: user-only Unix sockets and named pipes, the holders handover

How a program reaches a device served by another process on the same PC (personal computer), most of all the
ww hub (SRV-13), which
holds the devices and serves every program of the user (TR-16). `runtime_dir` is the per-user directory sockets
and lock files live in; `listen`/`connect` open a Unix domain socket on Linux and macOS or a named pipe on
Windows (through `interprocess`), reachable by the same user only, and each end checks who is on the other side.
`listen_with` with a `PeerPolicy` serves other users too (a group, or everyone), with the socket's and its
directory's modes to match (SRV-46).
`HandoverRequest`/`HandoverAnswer` ask a direct holder of a device to hand it to the hub (CLT-43), frozen. The
preamble and the message framing on these sockets come from `ww_byte_stream`.

See the [main repo](https://github.com/vhrdtech/wire_weaver) for the full picture and
[documentation](https://vhrdtech.github.io/wire_weaver) for the step-by-step guide.

# wire_weaver_client

> The std client runtime: event loop, Commander, the dynamic client and property observation

The `std`-side runtime a generated client depends on: the event loop, `Commander`, the dynamic client (`facade`,
its quantities from `ww_quantity`) and property observation (`observe`). Host transports (USB, RTT (real-time
transfer), WebSocket, UDP, local socket, in-process, the ww hub) are `Transport` implementations behind their own
feature (`usb`, `rtt`,
`ws`, `udp`, `local_socket`, `in_process`, `hub`), selected by a generated client's `ClientConfig`. There is no
`no_std` client yet (CLT-14).

See the [main repo](https://github.com/vhrdtech/wire_weaver) for the full picture and
[documentation](https://vhrdtech.github.io/wire_weaver) for the step-by-step guide.

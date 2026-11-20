# observe

Observed properties end to end (SRV-18): `Observe` requests and `PropertyUpdate` events between the generated
server, polled with a fake clock, and the real host event loop (`tests_common`). Device-side tests send
requests raw through the `Commander`; host-side ones go through the generated client.

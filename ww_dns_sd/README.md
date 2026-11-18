# ww_dns_sd

Announces WireWeaver WebSocket servers on the LAN over DNS-SD (multicast DNS) as service type `_wireweaver._tcp`, so clients find them without a typed address.

- `ServiceRecord`: instance name, port, addresses and the TXT keys `path`, `api`, `wire`, `id`, checked against DNS limits.
- `Advertiser`: keeps a record announced, follows address changes, withdraws it on drop.
- `MdnsResponder` (feature `mdns`, default): in-process, `mdns-sd` on its own thread.
- `AvahiResponder` (feature `avahi`): the system's Avahi daemon over D-Bus.

No async runtime needed. Everything announced reaches the whole LAN, unauthenticated. Docs: `docs/transport/websocket.md`, section "Announcing on the LAN".

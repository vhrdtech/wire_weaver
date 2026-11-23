# dns_sd

Tests DNS-SD announcements end to end (TR-32): `ww_dns_sd`'s responders, driven through an `Advertiser`, seen from the other side.

The in-process responder is browsed by a second `mdns-sd` daemon on loopback, and by `ww_dns_sd::Browser` (CLT-82); the Avahi responder talks to a fake Avahi over a peer-to-peer D-Bus connection.

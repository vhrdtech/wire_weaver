## Transport protocols

Several transport protocols are supported:

* USB (nusb on host side, embassy on embedded, no drivers needed on Windows/Mac/Linux)
* RTT over a debug probe (rtt-target on embedded, probe-rs on host side), see [RTT](rtt.md)
* WebSocket (host side, for devices on a network), see [WebSocket](websocket.md)
* UDP (host side, for devices on a network, best for telemetry), see [UDP](udp.md)
* TODO: CAN Bus (using CANOpen)

Others could be easily implemented, possibly reusing the same code.

USB and UDP transports support multiple events per packet/datagram. Many small messages can be accumulated over a time
window conserving bandwidth and allowing much higher message throughput per unit of time that would otherwise be
possible with one message per packet/datagram.

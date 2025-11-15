## Transport protocols

Several transport protocols are supported:

* USB (nusb on host side, embassy on embedded, no drivers needed on Windows/Mac/Linux)
* WebSocket (for reliable control access)
* UDP (for telemetry)
* TODO: CAN Bus (using CANOpen)

Others could be easily implemented, possibly reusing the same code.

USB and UDP transports support multiple events per packet/datagram. Many small messages can be accumulated over a time
window conserving bandwidth and allowing much higher message throughput per unit of time that would otherwise be
possible with one message per packet/datagram.

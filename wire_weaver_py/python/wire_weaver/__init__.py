"""Talk to any WireWeaver device, with its API built at runtime from introspection data; see `_wire_weaver` (the
compiled extension) for the actual implementation.

```python
import wire_weaver as ww

dev = ww.connect(vid_pid=(0xc0de, 0xcafe))
dev                                  # resource tree with signatures
dev.led_on()
dev.channel[1].gain.write(0.5)
with dev.samples as samples:
    for s in samples: ...
```

`connect()` goes through the ww hub when it already holds the device or `WW_HUB`/the open counter say so (CLT-39,
`docs/transport/hub.md`); the hub itself, started from this wheel with `python -m wire_weaver.hub` when no other one
is found, is reached as a device like any other through `hub()`.
"""

from ._wire_weaver import (
    Array,
    Device,
    HUB_VERSION,
    Method,
    Node,
    Observer,
    PositionedStream,
    Property,
    Quantity,
    RemoteError,
    SessionLost,
    Sink,
    Stream,
    WireWeaverError,
    __version__,
    connect,
    hub,
    list_devices,
    load_api,
)

__all__ = [
    "Array",
    "Device",
    "HUB_VERSION",
    "Method",
    "Node",
    "Observer",
    "PositionedStream",
    "Property",
    "Quantity",
    "RemoteError",
    "SessionLost",
    "Sink",
    "Stream",
    "WireWeaverError",
    "__version__",
    "connect",
    "hub",
    "list_devices",
    "load_api",
]

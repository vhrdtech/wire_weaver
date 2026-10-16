---
level: advanced
---

# Python: streams, observing, offline

More of the `wire_weaver` Python module, after the basics on the [Python](python.md) page: counted and reliable
streams, observing a group of properties, and working with an API crate without a device.

## Counted and reliable streams

A stream marked counted or reliable in the API (its signature says so) gives every item a position, reports what
was lost and where, and can be resumed from a saved cursor ([Stream reliability](api/stream_reliability.md)):

```python
s = dev.samples
s.open(rate=1000, ack="explicit")      # the stream function's arguments; returns its result, an Err raises RemoteError
                                       # open(cursor=saved) after a restart; ack="receive" is the default
while True:
    ev = s.recv_event(timeout=1)       # dict: kind "item", "gap", "resumed", "backfill_done", "disconnected", "closed"
    if ev["kind"] == "item":
        write(ev["pos"], ev["t"], ev["value"])  # t: device time in seconds, or None
        s.ack(ev["pos"] + ev["units"])          # explicit: the device may free it now
        save_cursor(s.cursor)                   # (session, pos), with the data
    elif ev["kind"] == "gap":
        print(f'{ev["units"]} lost at {ev["at"]} ({ev["cause"]})')  # device, link, store or host
s.stats                                # received, lost by cause, duplicates, queued ...
```

`recv()` and iterating return values only, gaps are counted in `stats`. A resumed reliable stream delivers the
latest values at once (`run` `"live"`) and the backlog beside them (`run` `"backfill"`), each in order.

## Observing

A trait level's `+observe` properties ([Observing properties](api/observing.md)) can be watched as a group, from
introspection alone (TOOL-11):

```python
m = dev.motor
m.observable_properties()                          # {"speed": "u32", "current": "u32", "position": "u32"}
with m.observe({"speed": 10, "current": None}) as obs:  # speed periodically every 10ms, current on change
    for u in obs:
        print(u["values"], u["t"], u["missed"])     # {"speed": 1500, "current": 3}, seconds or None, due sends that missed budget
obs.budget                                          # bytes/s the device applies to the whole level
```

`observe()` without `cfg` asks for every observable property on-change (what [`ww observe <path>`](cli_reference.md#ww-observe) does without
`--rate`). `recv(timeout=...)` / `try_recv()` are also available. One device subscription is shared per level
([One subscription per level](api/observing.md#the-host-side)); dropping the last observer (garbage collection)
unsubscribes.

## API from source, offline use

A device with introspection disabled needs its API from the crate source:

```python
api = ww.load_api("path/to/my_api", "MyApi")   # trait name is needed if the crate has several
dev = ww.connect(api=api)
```

`load_api()` alone works without a device: browse the API the same way, and serialize values to see what goes over
the wire, or decode captured bytes:

```python
>>> api = ww.load_api("tests/dynamic_api", "Dynamic")
>>> api.add.encode_args(40, b=-2).hex()
'28000000feff'
>>> api.speed.encode(1500)
b'\xdc\x05'
>>> api.speed.decode(b"\xdc\x05")
1500
>>> api.check.decode_return(b"\x00\x00\x01")
{'Err': 'TooBig'}
```

Methods have `encode_args()` / `decode_return()`, properties `encode()` / `decode()`, streams `decode()` and sinks
`encode()`. Calling a
resource of an offline API raises `WireWeaverError`.

## Not supported yet

- `f16`, `UN`/`IN` and LEB128 numbers.
- Typed (non-integer) array indices, property write errors (not decoded, same as generated clients).
- Observing a level inside a resource array ([Limits](api/observing.md#the-host-side)).
- asyncio: calls block the calling thread (with the GIL released).

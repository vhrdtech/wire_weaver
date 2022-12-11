# xPI Wire Formats

General purpose [wire formats](./wire_formats.md) are used with xPI data structures as well, yielding the same pros and
cons. On top of that, there is almost a zero cost mechanism to determine which one is being used, without prior
knowledge. To distinguish between different formats, -x is added:

* Binary xPI Dense - `xwfd`
* Binary xPI Sparse - `xwfs`
* Binary xPI Padded - `xwfp`
* Other formats can be used, but wasn't implemented due to lack of need.

# Binary xPI Dense (`xwfd`)

> no_std, no alloc, zero copy, space efficient implementation of xPI based
> on variable length encoding and buffer with 4 bits elements.

Nibble (4 bits) level access to buffers are used to save substantial amount of space for lower bandwidth channels (CAN
Bus, USART, I2C, etc).

First 4 bytes of serialized data structures are directly mappable to 29bit CAN ID. This optimization not only saves
additional space, but allows to utilize hardware filters available in many CAN controllers. It is also possible to use
different underlying interface, just treating serialized data as one continuous buffer. Layout is similar to uavcan, up
to bit 23, which is reserved = 0 in its specification. Here 1 is used, which will cause xwfd frames to be discarded by
uavcan stack.

## Event

| B0           | B1           | B2          | B3         | n8        | n9  | n10..     | ..            | ..    | ..       | last byte   |
|--------------|--------------|-------------|------------|-----------|-----|-----------|---------------|-------|----------|-------------|
| header 31:24 | header 23:16 | header 15:8 | header 7:0 | xwfd_info | ttl | node_set? | resource_set? | args? | padding? | req_id (5b) |

### Event header (32b)

| 31:29 (3b) | 28:26 (3b) | 25:24 (2b) | 23                | 22:16 (7b) | 15:7 (9b)    | 6:4 (3b)          | 3:0 (4b) |
|------------|------------|------------|-------------------|------------|--------------|-------------------|----------|
| n/a        | priority   | kind5:4    | is_xwfd_or_bigger | src        | dst_node_set | resource set kind | kind3:0  |

### Event kind (6b):

Event kind discriminant values are assigned in such a way, that different logical groups (requests, replies, multicast,
other)
can be easily distinguished with bits 5 and 4. I.e. for all requests kind5:4 = 00, for all replies kind5:4 = 01,
for all multicast/broadcast kind5:4=10 for all other kind5:4=11.
This might be helpful if hardware filters are to be used.

### Resource set kind:

* 0: One segment of 4 bits (/0..=15)
* 1: Two segments of 4 bits (/0..=15 /0..=15)
* 2: Three segments of 4 bits (/0..=15 /0..=15 /0..=15)
* 3: Three segments of 6, 3 and 3 bits (/0..=63 /0..=7 /0..=7)
* 4: Three segments of 6, 6 and 4 bits (/0..=63 /0..=63 /0..=15)
* 5: Any number of segments as vlu4 array
* 6: MultiUri (see below)
* 7: Reserved

## Compatibility between wire formats

There is a mechanism to determine which wire format is being processed. However, it is not required to support all of
them, and it is possible to discard unsupported data without processing.
Space is saved in favor of more constrained wire formats: for potential bit level format only one bit is reserved.
For `xwfd` additional nibble is reserved. For `xwfs` and `xwfp` additional byte is

Decision process:

1. Read MSB bit of the second byte (bit 23 of the first word) = is_xwfd_or_bigger
   * if is_xwfd_or_bigger == 1 => read additional nibble in byte 5, bits 7:4 = xwfd_info
   * if is_xwfd_or_bigger == 0 => reserved value for potential bit level wire format, discard.
2. Check xwfd_info to determine whether format is xwfd or not
3. TBD

### xwfd_info (4b):
* 0000: xwfd
* _: reserved
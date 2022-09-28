# xPI Wire Formats

General purpose [wire formats](./wire_formats.md) are used with xPI data structures as well, yielding the same pros and
cons. On top of that, there is almost a zero cost mechanism to determine which one is being used, without prior
knowledge. To distinguish between different formats, -x is added:

* Binary xPI Dense - `xwfd`
* Binary xPI Sparse - `xwfs`
* Binary xPI Padded - `xwfp`
* Other formats can be used, but wasn't implemented due to lack of need.

# Binary xPI Dense

> no_std, no alloc, zero copy, space efficient implementation of xPI based
> on variable length encoding and buffer with 4 bits elements.

Nibble (4 bits) level access to buffers are used to save substantial amount of space for lower bandwidth channels (CAN
Bus, USART, I2C, etc).

First 4 bytes of serialized data structures are directly mappable to 29bit CAN ID. This optimization not only saves
additional space, but allows to utilize hardware filters available in many CAN controllers. It is also possible to use
different underlying interface, just treating serialized data as one continuous buffer. Layout is similar to uavcan, up
to bit 23, which is reserved = 0 in its specification. Here 1 is used, which will cause vlu4 frames to be discarded by
uavcan stack.

## Request

| 31:29 (3b) | 28:26 (3b) | 25:24 (2b)                | 23          | 22:16 (7b) | 15:7 (9b)   | 6:4 (3b)          | 3:0 (4b)     |
|------------|------------|---------------------------|-------------|------------|-------------|-------------------|--------------|
| n/a        | priority   | event kind = request (11) | is_vlu4 = 1 | source     | destination | resource set kind | request kind |

## Compatibility between wire formats

There is a mechanism to determine which wire format is being processed. However, it is not required to support all of
them, and it is possible to discard unsupported data without processing.

MSB bit of the second byte = 1 (bit 23 of the first word), means that wire format is vlu4.
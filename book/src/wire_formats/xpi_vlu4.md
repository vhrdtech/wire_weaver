# xPI vlu4 wire format

> no_std, no alloc, zero copy, space efficient implementation of xPI based
> on variable length encoding and buffer with 4 bits elements.

Nibble (4 bits) level access to buffers are used to save substantial amount of space for
lower bandwidth channels (CAN Bus, USART, I2C, etc).

First 4 bytes of serialized data structures are directly mappable to 29bit CAN ID.
This optimization not only saves additional space, but allows to utilize hardware filters available in many CAN
controllers.
It is also possible to use different underlying interface, just treating serialized data as one continuous buffer.
Layout is similar to uavcan, up to bit 23, which is reserved = 0 in its specification. Here 1 is used, which will cause
vlu4 frames to be discarded by uavcan stack.

Request
---

| 31:29 (3b) | 28:26 (3b) | 25:24 (2b)                | 23          | 22:16 (7b) | 15:7 (9b)   | 6:4 (3b)          | 3:0 (4b)     |
|------------|------------|---------------------------|-------------|------------|-------------|-------------------|--------------|
| n/a        | priority   | event kind = request (11) | is_vlu4 = 1 | source     | destination | resource set kind | request kind |

Compatibility with other wire formats
---
Setting bit 23 = 0 indicates that different wire format is used and that further data is to be processed according to
byte 5 and 6.

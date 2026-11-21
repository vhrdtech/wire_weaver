# ww_byte_stream

> WireWeaver over byte streams: the connection preamble and message framing

The first bytes of every WireWeaver connection over a byte stream (a local socket, TCP, a process's stdio, any
stream a program accepted) and the framing of `ww_link` messages after them. `Preamble`/`Answer` pick what the
connection is for and are frozen; `MessageReader`/`MessageWriter` carry `| len: u32 LE | kind: u8 | payload |`
messages.

See the [main repo](https://github.com/vhrdtech/wire_weaver) for the full picture and
[documentation](https://vhrdtech.github.io/wire_weaver) for the step-by-step guide.

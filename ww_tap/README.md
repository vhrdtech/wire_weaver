# ww_tap

> Link tap: the hook a transport reports frames, messages and link events of a WireWeaver link to

What crosses a WireWeaver link, recorded at the transport edge, for monitors, statistics and captures (TR-14). A
transport calls a `Tap` at the two layers it has: `Tap::frame` with the bytes as they cross the medium, and
`Tap::message` with every `ww_link` message as it goes down to the framer or comes back up; the event loop adds
what never shows in bytes with `Tap::link_event`. The tap only copies bytes and stamps time; linking frames to
messages and decoding happen in the consumer (`wire_weaver_client::tap`). `NoTap` is the default everywhere, its
calls empty and inline, so a build without a tap compiles them away. With the `std` feature, `tape` defines a
link's `ww_tape` tracks and the tap clock of a capture, and finds them again.

See the [main repo](https://github.com/vhrdtech/wire_weaver) for the full picture and
[documentation](https://vhrdtech.github.io/wire_weaver) for the step-by-step guide.

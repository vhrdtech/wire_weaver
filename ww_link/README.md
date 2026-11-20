# ww_link

> WireWeaver link layer: message kinds and their payloads carried over ww_framer frames

Sans-IO and `no_std`: knows how to encode a `Message` into framer messages and decode them back, nothing more.
Sending frames, timers and connection state are up to the host (`wire_weaver_client`) or device event loops
(`ww_device`). Frame layout is defined by `ww_framer`: each frame holds one or more link messages, each prefixed
with a `U2Head` carrying a `Kind` as its `user_kind`; data messages use the compact 2-bit form, the rest the
extended one. Depends on `shrink_wrap` and `ww_framer`.

See the [main repo](https://github.com/vhrdtech/wire_weaver) for the full picture and
[documentation](https://vhrdtech.github.io/wire_weaver) for the step-by-step guide.

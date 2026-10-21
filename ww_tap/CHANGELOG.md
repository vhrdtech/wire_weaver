# ww_tap changelog

## Unreleased

### ⚠️ Breaking

- `Framer::U2` has a `prefix: FramerPrefix` field (`None`, `Seq`; `#[default = None]`, so older sessions still decode) and `FramerChecksum` a `Crc32IsoHdlc { full }` variant, for `ww_link`'s UDP framing and the USB frame counter diagnostic (TR-15). Code building `Framer::U2` adds `prefix`.

### 🐛 Fixes

- `Recorder::add_bundle` hands the open block and a describe block with the bundle to subscribers at once: a live reader got the bundle after the messages that needed it.

### 🚀 Features

- First version (TR-14): the link tap. `Tap` (`session`, `frame`, `message`, `link_event`, `wants_frames` / `wants_messages`) taken by transports at their edge, `NoTap` the default with empty inline calls; the records `Session` (side, medium, framer configuration, frame size, peer, the device's `DeviceInfo` once set up), `Frame`, `Message`, `LinkEvent` (`Connected`, `LinkReady`, `Disconnected`, `Error`, `Lost`) and `ClientMap` as ShrinkWrap types, streams of the `LinkTap` / `Link` traits so a capture is described by the crate's API bundle (snapshot `ww_tap_0_1_0.ron`). With `std`: `Recorder` batches records into pooled `ww_tape` blocks (`RecorderConfig::block_size` 64 KiB, `flush_time` 10 ms, `pool_blocks`, `describe_every`), hands a block over when full or on time with only its used bytes, allocates nothing in the steady state, counts records lost while consumers hold every block in a `LinkEvent::Lost`, and writes describe blocks (tracks, bundles from `add_bundle`, the newest session per link) when a consumer subscribes or a link is added; `Recorder::link` gives a link's `RecorderLink`, `subscribe` (bounded queue) and `subscribe_fn` the consumers, `with_fake_clock` / `poll_flush` for tests.

# ww_tap changelog

## Unreleased

### 📚 Documentation

- A README.

### 🚀 Features

- `Recorder::tape_clock`: a `ww_tape::ClockRef` on the recorder's clock, so a storing writer syncs on fake time in tests.
- `LinkKey { medium, peer }`: the key of a link named by its peer, made from its first `Session` (TR-14).
- `api()` (same name and version as before) and `ids::stream_name` without `std`, for producers outside the crate (TR-14).

- `ConsumerStats` on `Subscription::stats` and `Recorder::consumers`: blocks missed, records lost, behind since when (STD-135).
- A consumer's next block after a miss comes after a `LinkEvent::Lost` of each link it lost records of (STD-135).
- `recorder::drop_closed_links`: a relay drops a closed link's tracks from its `Definitions` (STD-135).

### 🐛 Fixes

- The recorder shares its definitions and sessions with the puts it makes, and makes none for a full queue (STD-135).

### ⚠️ Breaking

- Declares `rust-version = "1.99"`: cargo of an older Rust refuses the crate.
- Built on ww_tape 2bfdd07: recorder tracks carry the piece's owner (zeros) and no relations; head and index rows changed (ww_tape TAPE-8).
- The recorder writes tape format 1: a session is a state record, put to consumers that lack it (STD-135).
- Each consumer is put only what its next block needs and lacks within near; no producer record is sent (STD-135).
- A consumer whose queue can't take a block gets the puts that fit, a block at a time, and misses the block (STD-135).
- `LinkEvent::Closed` ends a link's records; `subscribe` queues hold at least `QUEUE_MIN` (2) blocks (STD-135).
- The recorder's blocks carry a random run tag per recorder, counted from seq 0 (STD-129).
- Track keys hash the name path `link[i].<stream>` and the crate name only: tracks get new ids (STD-125).
- The recorder's tracks are `TrackKind::Events` instants in the describe set (STD-127).
- Tape v2 (STD-117): record times count `HOST_CLOCK` (ns since the recorder started), mapped to `WALL_CLOCK` in the describe set; `RecorderConfig::describe_every` is gone: describe blocks are no longer repeated by time.

- `Framer::U2` has a `prefix: FramerPrefix` field (`None`, `Seq`; `#[default = None]`, so older sessions still decode) and `FramerChecksum` a `Crc32IsoHdlc { full }` variant, for `ww_link`'s UDP framing and the USB frame counter diagnostic (TR-15). Code building `Framer::U2` adds `prefix`.

### 🐛 Fixes

- `Recorder::add_bundle` hands the open block and a describe block with the bundle to subscribers at once: a live reader got the bundle after the messages that needed it.

### 🚀 Features

- `track_key(link, stream)`: the stable key of a link's stream track (STD-125).
- `Medium::InProcessWebSocket` for sessions over `wire_weaver_client`'s simulated WebSocket (TR-22), appended.
- `Medium::InProcessUdp` for sessions over `wire_weaver_client`'s simulated UDP (TR-21), appended, so older captures still decode.
- `Medium::LocalSocket` for sessions over a local socket (TR-16), appended, so older captures still decode.
- First version (TR-14): the link tap. `Tap` (`session`, `frame`, `message`, `link_event`, `wants_frames` / `wants_messages`) taken by transports at their edge, `NoTap` the default with empty inline calls; the records `Session` (side, medium, framer configuration, frame size, peer, the device's `DeviceInfo` once set up), `Frame`, `Message`, `LinkEvent` (`Connected`, `LinkReady`, `Disconnected`, `Error`, `Lost`) and `ClientMap` as ShrinkWrap types, streams of the `LinkTap` / `Link` traits so a capture is described by the crate's API bundle (snapshot `ww_tap_0_1_0.ron`). With `std`: `Recorder` batches records into pooled `ww_tape` blocks (`RecorderConfig::block_size` 64 KiB, `flush_time` 10 ms, `pool_blocks`, `describe_every`), hands a block over when full or on time with only its used bytes, allocates nothing in the steady state, counts records lost while consumers hold every block in a `LinkEvent::Lost`, and writes describe blocks (tracks, bundles from `add_bundle`, the newest session per link) when a consumer subscribes or a link is added; `Recorder::link` gives a link's `RecorderLink`, `subscribe` (bounded queue) and `subscribe_fn` the consumers, `with_fake_clock` / `poll_flush` for tests.
- Wire version `w1.0` of its own (EVO-26), in `Cargo.toml` as `[package.metadata.wire_weaver] wire = "w1.0"`: `#[since]` in this crate names its revisions, and a Rust-only break of the crate no longer touches the wire line of the APIs using it. The snapshot carries it (`wire`, `ext_wire`), so its hash changed.

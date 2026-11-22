---
level: advanced
---

# The link tap

The link tap is the hook a transport reports what crosses a WireWeaver link to: every frame as it crossed the medium
(a USB packet, a UDP datagram, an RTT chunk), every `ww_link` message as it went down to the framer or came back up,
and the link events the bytes don't show (connected, link ready, disconnected, errors). What to do with them is the
tap's: a decoder links frames and messages back together, so you see which packet a request went out in, which
packets a long reply was split over, and what a lost packet took with it. WireWeaver itself ships no recorder and no
`ww` command on the tap; a capture tool implements `Tap` and stores the records in its own format.

## Where it records

The tap is the `ww_tap` crate (`no_std`). A transport calls its `Tap` at the two layers it has: `frame(dir, bytes)`
and `message(dir, kind, bytes)`; the event loop adds `link_event`. The tap only copies bytes and stamps a time:
linking, statistics and decoding happen in the consumer, so the link pays for a copy and nothing more.

- **Host.** Every `wire_weaver_client` event loop reports its link to a `Tap` made for that link by the
  `tap::TapSource` of `ClientConfig::tap(source)`: `TapSource::new(|| Box::new(my_tap))` for any `Tap`. A client
  without a tap copies nothing; a tap that panics is dropped and its link goes on (`TapSource::on_lost`). `Command::RegisterTracer` and
  its `TraceEvent`s come from the same hook: the data messages each way and the link's connects, disconnects and
  errors.
- **Device.** `ww_device`'s `FramedTx::with_tap` / `FramedRx::with_tap` take a tap; the default is `NoTap`, whose
  calls are empty and inline, so firmware without a tap is the same size as before (checked on an STM32G0 build).
  `ww_framer` itself has no callbacks: `ww_framer::pieces(frame)` walks a frame into its pieces with the same head
  and checksum code `FramedRx` uses, and the decoder re-runs the framer on the recorded frames.

## Records

The records are ordinary WireWeaver types: streams of the `ww_tap::LinkTap` trait, one set per link: `session`
(medium, framer, frame size, peer, the device's `DeviceInfo` once the link is set up), `events`, `tx_frames`,
`rx_frames`, `tx_messages`, `rx_messages` and, on a server, `client_map` (which client's request a device-side
request was). A capture is described by the API bundle of `ww_tap` like any device log is by its device's, and every
frame and message has a number, so the decoder sees a missing one.

A link that closes records `LinkEvent::Closed` on its events track. A relay that keeps the tap's definitions drops
that link's tracks when it sees the event (`ww_tap::drop_closed_links`), so its definitions don't grow with every link
ever seen. A producer that could not keep records says so with `LinkEvent::Lost { records }`: a gap is visible, never
silent.

## Links and devices

Every link's tracks of one stream have the same key, the hash of the stream's name (`ww_tap::track_key`). The link's
identity tells them apart: a producer defines a link's tracks at its first session (`ww_tap::define_link_tracks`, on
the tap clock of `ww_tap::define_clocks`), with the [track
dimensions](https://tape.vhrd.tech/tracks/#number-key-and-id) `medium` and `peer` (`ww_tap::dims`), the USB serial
or the address the session reports. Two devices of one API recorded into one capture thus give two track ids per
stream with one key, and the same device gives the same ids in every capture and after a reconnect, whichever link
number the producer gave it.

A link the peer doesn't tell apart, with no peer or with the peer of another open link, adds the dimension `link`,
the producer's number of it; so does a link recorded before any session.

A reader picks a device by its dimension: `ww_tap::link_stream_of` gives a tap track's stream and its link's
`LinkIdentity` (`identity.get(dims::PEER)`), and the decoder numbers links by identity (`Decoder::find_link(dims::PEER,
"SN-1234")`, `Decoder::link_identity`).

## Capture files

A capture is tape blocks appended back to back. The tape format has a book of its own, the [ww_tape
book](https://tape.vhrd.tech/): what a file holds, how time is stored and what survives damage, with a
[format reference](https://tape.vhrd.tech/format/) of its parts. The definitions carry the device's API bundle, so
the decoder explains the requests and replies of a capture on any PC, without the device. A track's key hashes its
resource names and its dimensions name the device, so two runs give the same ids ([Links and
devices](#links-and-devices)).

## Decoding

`wire_weaver_client::tap::Decoder` takes a link's records (`Decoder::record`, `Decoder::session`) or the blocks of a
capture (`Decoder::feed`, from `ww_tape::FileBlocks`), and gives out `Decoded` frames, messages and link events:

- **Linkage.** Every frame is walked into its pieces and reassembled with the framer's rules: a message knows the
  frame bytes it came in (`MessageInfo::frames`, `#5@2+62`: frame 5, offset 2, 62 bytes), a frame its pieces and
  what became of them (`FrameInfo::outcome`), a lost frame shows as a gap in the frame numbers and as the split
  message it broke.
- **Cross-check.** Each message the framer gives back is compared with the one the transport recorded: a
  `Check::Mismatch` is a framer bug.
- **The API.** Data messages are read as requests and events and explained with the device's API bundle (from the
  capture, or the local cache); a reply is paired with its request by `seq`, so its return value is explained by
  its method (no `--for` as with `ww explain event HEX`).
- **Statistics.** `Decoder::stats` per link and direction: frames, fill, overhead, messages per frame, split
  messages, dropped pieces, frame gaps, mismatches, message-to-frame latency, link message kinds, and per method its
  calls and reply times; `Stats::report()` prints them.

Live records and saved blocks go through the same code, so a capture decodes exactly as the live session did.

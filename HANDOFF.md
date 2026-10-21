# HANDOFF: framer prefix + link setup token (TR-15)

Design: `~/git/tpm/prj/P2401-wireweaver/design/framer_frame_seq.md` (round 2, decided 21 Oct 2026), pieces 1-4.

## Done

- Piece 1 (commit "feat(framer): frame prefix ..."): `ww_framer::prefix` (`FramePrefix`, `NopPrefix`, `SeqPrefix`, `SeqRx`, `FrameReport`, `PrefixStats`), 4th generic param `P = NopPrefix` on `Tx`/`FramedRx`/owned/`Pieces`; `Tx::new_session()`; `FramedRx::stage`/`commit_staged` return `FrameReport`. Checksums now cover frame prefix + head of the piece carrying them (fuzz found a flipped user-kind bit delivered under the wrong kind). `CrcAlgorithm` is a fn now. Fuzz target `framer-lossy` (12M runs / 15 min clean). Docs ww_framer.md, ww_framer_wire.md.
- Piece 2 (commit "feat(link): random setup token ..."): token in GetDeviceInfo/LinkSetup, echoed in DeviceInfo/LinkReady; host drops all before its DeviceInfo; device `Transmit::NewSession` + `MessageTx::new_session()`; `Error::IncompatibleLinkVersion`. E2E `previous_session_leftovers_are_dropped`. Docs ww_link.md "Setup token".

## Next (piece 3, then 4)

- ww_link: `UsbChecksum = NopChecksum`, `UsbPrefix = NopPrefix` (+ option `SeqPrefix`), `UdpChecksum = CrcChecksum<BlockCrc32, true>` via `ww_block::crc32_update` (Roman: ww_block's CRC-32 is the one implementation; ww_link std -> ww_block crc-table-slice16), `UdpPrefix = SeqPrefix`.
- ww_device FramedTx/FramedRx are hardcoded to USB config types: make the framing a type param (a `Framing` trait in ww_link with Head/Checksum/Tail/Prefix). UDP device side + `starts_link_setup` must skip the prefix. Device rx resync on takeover (reset already resyncs).
- Host: usb/event_loop.rs, udp.rs, in_process.rs use the configs; counters (`prefix_stats`) into `ww stats`; tap: `ww_tap::Framer::U2` needs a prefix field + CRC-32 checksum variant; reassembly.rs walker with prefix; `ww monitor` shows prefix per frame.
- dfu_scenarios transcripts change again (USB without CRC): `just dfu-transcripts`.
- Piece 4: usb_internals.md, udp.md, evolution note; FEATURES TR-15 (claimed) + tests/features_page/entries.md + `just features-page`; flash/stack numbers on thumbv8m for the device side; just test + just lint.

## Decisions / gotchas

- No version bump (memory rule; workspace 0.5.0 already > crates.io 0.4.0). Older 0.5.0-dev peers are detected by the missing token (reads 0), not by version.
- SeqRx: epoch +1/+2 = new session, -1 = stale; resync only on two consecutive duplicate-looking frames that follow each other, and that frame is dropped. Order guaranteed unless a frame was corrupted (corrupted prefix can shift the reference before CRC rejects the frame).
- Device resets tx framer on every GetDeviceInfo (also retries); host keeps one token per connect.

# HANDOFF: framer prefix + link setup token (TR-15)

Design: `~/git/tpm/prj/P2401-wireweaver/design/framer_frame_seq.md` (round 2, decided 21 Oct 2026), pieces 1-4.

## Done

- Piece 1 (6a4a24ef): `ww_framer::prefix`, checksums over prefix + head, fuzz target `framer-lossy`.
- Piece 2 (a042a15d): setup token, device framer reset, `Error::IncompatibleLinkVersion`.
- Fix (c115560a): host rx task drained ToRx before a transport error (keep_streams race, found by sink_e2e).
- Piece 3 (f00db6c3): `ww_link::Framing` (UsbFraming, UsbSeqFraming, UdpFraming), LinkCrc32 = ww_block CRC-32, PacketSink/Source::Framing, USB interface protocol 0x37/0x38 picks the host framing, `wire_weaver_usb_embassy` feature `seq-prefix`, tap prefix + CRC-32, `ww stats` PrefixCounts, `ww monitor` prefix, transcripts.

## Next: piece 4

- Docs: docs/transport/usb_internals.md, udp.md, ww_link.md (Framing, per medium table), ww_framer.md mention, evolution note (wire break of USB/UDP link: which devices/hosts).
- FEATURES.md TR-15 (claimed) done + tests/features_page/entries.md + `just features-page`.
- Ask before `just build-docs` (user may run serve-docs).
- Then just test + just lint, `git rm HANDOFF.md`, tpm work ready.

## Numbers (for the report)

- QEMU M3 framer_rx (opt 3, 60 msgs over 64-B frames): master USB (CRC-16) 75,202 insns / 59 frames; now USB 54,867 / 58; USB seq 61,882 / 59; UDP 233,108 bitwise CRC-32, 96,218 crc-table-256, 98,618 slice16 / 63 frames.
- RP2350 thumbv8m opt z, text/bss: usb_blinky master 58164/12288, after pieces 1-2 59692/12336, now 58908/12336. udp_ncm master 104364/44312, pieces 1-2 106160/44392, now 107236/44472.
- Stack (-Z emit-stack-sizes): ww_server task poll 1296 -> 1288 B (USB), 1304 -> 1304 (UDP); FramedRx wait 152 -> 160 (USB), 128 -> 144 (UDP). Pre-existing: rp2 udp_ncm main task frame 32,600 B (not ours).

## Gotchas

- base worktree /tmp/fp-base (detached) used for before numbers; remove with `git worktree remove /tmp/fp-base` at the end.

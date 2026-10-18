# HANDOFF: ww-tap (P2401#18oct-2157, TR-14 link tap, TOOL-14 CLI, STD-42 ww_logbook)

## Done (steps 1-2)

- iceoryx2 USB tracer, `usb-tracing` and the unused `ipc` feature removed (Roman, 22:20).
- `ww_framer::pieces::<H, C, T>(frame)`: shares `read_head` / `Extent` with `FramedRx::process_next`. Measured: `examples_mcu/mcu_qemu` `qemu-framer-rx` 76039 -> 75202 instructions (M3, opt 3); G0 usb_blinky +24 B.
- `ww_logbook` (STD-42, generic, Roman 22:24: own crate in this workspace, logring builds on it): block header 32 B + varint records + CRC-32C, variable length, never padded; describe records (`Producer`, `TrackDef`, `Bundle`); `FileBlocks` reader.
- `ww_tap` (TR-14): no_std `Tap` / `NoTap`, records as streams of `#[ww_trait] LinkTap { link[]: Link }` (snapshot `ww_tap_0_1_0.ron`, in `just save-snapshots` and the snapshot test); std `Recorder` (pooled blocks, `flush_time` 10 ms, `describe_every` 1 s, lost records -> `LinkEvent::Lost`, `with_fake_clock` + `poll_flush` for tests). Track number = 1 + link*7 + stream id.
- `ww_device` `FramedTx::with_tap` / `FramedRx::with_tap`; NoTap: G0 firmware identical size (record tx frames before the await, never use tap data after an await: it grows the future).
- `wire_weaver_client`: `Transport::connect(handle, tap)`, transports record session + frames, worker records messages, cores link events; `ClientConfig::tap`, `tap::default_recorder()`; `RegisterTracer` = `tap::trace_events` adapter. Workspace tests green.

## Next

3. Decoder (`wire_weaver_client::tap::decode`): reassembly over `pieces` -> per-message `(frame_no, offset, len)` and per-frame messages, cross-check vs Message records, framer + link stats, seq pairing into `envelope::explain_event` (no `--for`). Session.framer -> concrete framer types (USB/UDP: U2Head + CrcChecksum<Crc16IbmSdlc>; RTT stream: U2Head, NopChecksum, stream chunks need carry-over).
4. CLI: `ww monitor [--layer frame|link|api]`, `ww stats`, `ww capture -o FILE` (file = blocks appended; add LinkTap bundle from `wire_weaver_snapshots` + device bundle via `Recorder::add_bundle`), `ww explain FILE` (WHAT is an existing file starting with `WWLB`). A `Feed` seam (where blocks come from) for the later server. Completion, colour, help examples, skill tree / P2618.
5. E2E over `in_process::usb_device` (64 B): split messages, several per frame, a lost frame (needs a lossy hook on the simulated USB link), stats; capture replays identically. FEATURES TR-14/TOOL-14/STD-42 + `tests/features_page/entries.md` + `just features-page`; docs page; `just test`, `just lint`; `git rm HANDOFF.md`; `tpm work ready`.

## Decisions and gotchas

- Describe blocks (not pooled) at subscribe, new link, after loss, every `describe_every`; data blocks carry only records (answer to Roman: only used bytes, no empty blocks, no padding in files).
- Capture files must carry the device's bundle per ApiHashPair (parent asked): add via `Recorder::add_bundle(hash, bytes)`.
- Firmware size comparisons must be built at the same path (panic locations, defmt strings).
- `Command` is 72 B now (guard raised from 64).

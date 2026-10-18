# HANDOFF: ww-tap (P2401#18oct-2157, TR-14 link tap, TOOL-14 CLI, STD-42 ww_logbook)

Ids: TR-14 (link tap), TOOL-14 (ww monitor/stats/capture/explain FILE), STD-42 (ww_logbook). Claimed, not yet written into FEATURES.md.

## Done (steps 1-3, all committed, workspace nextest green at 79de6e3b, client tests green at ee5591a4)

- iceoryx2 USB tracer, `usb-tracing` and the unused `ipc` feature removed (Roman, 22:20).
- `ww_framer::pieces::<H, C, T>(frame)`: shares `read_head` / `Extent` with `FramedRx::process_next`. `examples_mcu/mcu_qemu` `qemu-framer-rx` 76039 -> 75202 instructions (M3, opt 3); G0 usb_blinky +24 B.
- `ww_logbook` (STD-42, generic, Roman 22:24: own crate in this workspace, logring builds on it): 32 B header + varint records + CRC-32C, variable length, never padded; describe records (`Producer`, `TrackDef`, `Bundle`); `FileBlocks`, `BlockBuf`.
- `ww_tap` (TR-14): no_std `Tap` / `NoTap`, records = streams of `#[ww_trait] LinkTap { link[]: Link }` (snapshot `ww_tap_0_1_0.ron`, in `just save-snapshots` and the snapshot test); std `Recorder` (pooled blocks, `flush_time` 10 ms, `describe_every` 1 s, lost -> `LinkEvent::Lost`, `with_fake_clock` + `poll_flush`). Track number = 1 + link*7 + stream id (`track_number` / `track_of`).
- `ww_device` `FramedTx::with_tap` / `FramedRx::with_tap`; NoTap: G0 firmware identical size.
- `wire_weaver_client`: `Transport::connect(handle, tap)`; sessions + frames in transports, messages in the worker, link events in the cores; `ClientConfig::tap(recorder)`, `tap::default_recorder()`; `RegisterTracer` = `tap::trace_events` adapter (tests/observe* green).
- Decoder `wire_weaver_client::tap::{Decoder, Decoded, What, FrameInfo, MessageInfo, Check, ApiInfo, Stats}` (`tap/decode.rs`, `tap/reassembly.rs`, `tap/stats.rs`): linkage, cross-check, seq pairing (seq read even when the request doesn't decode), explain with bundle (describe `Bundle` records keyed by `ApiHashPair::no_docs.hash` bytes, or `with_local_cache()`), `Stats::report()`. E2E `tap/e2e_tests.rs` (64 B simulated USB, shared host packets, split 300 B replies, a lost device packet via `LossySink`, stats, captured blocks replayed through `FileBlocks` -> identical Debug output and stats; stress 30x green). `in_process::test_transport(path)` (cfg test).

## Next: step 4, the CLI (TOOL-14), then step 5 (finish)

Plan worked out:
- New `wire_weaver_cli/src/cmd/tap.rs`: `Layer` ValueEnum (frame | link | api, doc comment per value = completion help), a `Feed` trait (where blocks come from: `LiveFeed(Subscription)` now, `FileFeed(FileBlocks<File>)` for files, a ww server feed later, SRV-13) and a text renderer of `Decoded` (one line per frame / message / event: `t link dir frame #n len [pieces: kind uk len -> outcome]`, `msg #n kind len frames n@off+len.. title (reply to #m in x ms)`; api layer: data messages only, with the explained fields via explain.rs `render`). Colour with `console` (off without TTY / NO_COLOR).
- `ww monitor [--layer]`, `ww stats [--every S] [--duration S]`, `ww capture -o FILE [--duration S]`: main.rs must create the `Recorder` and `subscribe` BEFORE connecting and connect with `selection.client_config().tap(recorder)` (so link setup is captured); consumer loop in `spawn_blocking` (Subscription is blocking); Ctrl+C via tokio `signal` feature (add to CLI's tokio features) to flush and print a summary. Capture: append chunk bytes to the file, add bundles with `Recorder::add_bundle`: the LinkTap bundle from `wire_weaver_snapshots` (ww_tap 0.1.0) and the device's bundle from `DynClient::device_introspect()` (serialize `api_bundle` with `to_ww_bytes_owned`, key = `user_api_hash.no_docs.hash` bytes); indicatif spinner with bytes/blocks.
- Decoder fix to do: on a new describe `Bundle`, reset `root_tried` of all links (now a data message before the bundle leaves the link unexplained for good).
- `ww explain FILE`: in `explain.rs`, when no hex/--value/--file and WHAT is an existing file starting with `WWLB`, decode it with the same renderer (`--layer`, default link); `need_device()` false then. Without the server, monitor/capture show the CLI's own direct connection (link setup, introspection, pings).
- Test in the CLI crate (dev-dep `wire_weaver_client` features `in_process`, plus `ww_device`): render a live in-process session to a String, write the capture, `explain FILE` rendering equals it.
- Help examples in `cli.rs` doc comments (`verbatim_doc_comment`), add to `need_device` / tracing level match; skill tree signal for the 4 subcommands or a P2618 task (tpm CLAUDE.md "Skill tree").

Step 5: FEATURES.md TR-14 / TOOL-14 / STD-42 items (+ `tests/features_page/entries.md`, `just features-page`), a docs page (e.g. `docs/tools/tap.md`, level, `zensical.toml` nav; ask before `just build-docs`), CHANGELOGs (ww_framer, ww_logbook, ww_tap, ww_device, client done; CLI to add), `typos` words, `just test`, `just lint`, check-mcu / examples (ww_device change), `git rm HANDOFF.md`, `tpm work ready`.

## Decisions and gotchas

- Describe blocks (not pooled) at subscribe, new link, after loss, every `describe_every`; data blocks carry only records; only used bytes, no empty blocks, no padding in files (told Roman).
- Capture files carry the device's bundle per ApiHashPair (parent asked, confirmed).
- Firmware size comparisons must be built at the same path (panic locations, defmt strings). Never use tap data after an await in ww_device (grows the future even with NoTap).
- `Command` is 72 B now (guard raised from 64).
- Envelope titles: `Request {variant} {path}`, `Event {variant} {path}`; stats key requests by the title without `Request `.

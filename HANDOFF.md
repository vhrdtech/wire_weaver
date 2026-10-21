# Handoff: SRV-29 hub device multiplexing (session hub-tunnels, omarchy-m1)

Branch `s/21oct2026-hub-tunnels`, built on session ww-hub's `s/21oct2026-ww-hub` (efbf9646). The parent (ww-hub on gpd-omarchy) cherry-picks our commits when we say ready; their branch meanwhile has CLT-39 routing and TOOL-17 and a "What is there today" section in `docs/transport/hub.md` (expect a docs merge on their side, no need to rebase).

## Done (committed)

- `wire_weaver_client` (a70628dd and later): `Command::Forward { bytes, tag: Option<u64>, timeout }`, `Command::ForwardSink { sink: Box<dyn ForwardSink> }` (trait, called on the rx task; `UnboundedSender<Forwarded>` implements it), `Command::ForwardLoopback`, `Command::ForwardFlush`, `Forwarded { Reply { tag, event, last }, Event, Loopback, Failed }` (all in `internal`). Dispatcher: forwarded seqs in `forwards` map, seq-0 events to the sink. `ClientConfig::hub_device(key)` / `hub_device_at(path, key)`, `ClientConfig::holder_mode(Mode)` (USB/RTT handles carry it; hub uses `Mode::Hub`). `DeviceApiInfo::accumulation_time` (new pub field). Fix: a `LoopbackTest` sent during link setup is held until up (was "not connected" race; test `loopback_test_during_link_setup_starts_when_up`). CHANGELOG entry written for Forward/hub_device/holder_mode — still to add: ForwardSink trait change, ForwardFlush, accumulation_time, the loopback race fix (🐛).
- `ww_hub`: `envelope.rs` (seq split/replace, classify request/event), `tunnel.rs` (TunnelQueue: 1 MiB droppable data per client, replies never dropped; link endpoint on `ww_device::DeviceLink` with the device's DeviceInfo values and accumulation window; flushes to the device when the client's write batch ends), `device.rs` (open on demand by key `usb:`/`rtt:`/`inproc:`, `Device` with `tokio::Mutex<State>` shared by tunnels (requests go straight to the event loop) and the device task (non-direct answers, seq-0 fan-out, Join/Leave, linger); `Router` = ForwardSink on the rx task routing plain replies (tags with bit 63 = tunnel<<32|client seq) and loopback echoes straight into tunnel queues; stream open counted per path (first client opens on device, later get the cached open answer, last close closes; sidebands of a shared stream only from the first client), observe merged per level (`Level`, latest setting per property, max budget), introspection cached (seq-0 root-path stream data), `Registry::take`, linger (`HubConfig::linger`, `WW_HUB_LINGER`, 30 s), hub shutdown closes devices. `devices::entries` shows hub-held devices as `Open` with tunnels. ww_hub CHANGELOG entry written.
- `tests/hub` + `tests/hub_api` (new, publish = false): 6 e2e tests all green (calls remapped across two clients, shared stream opened once/closed by last, client killed mid-stream, introspection from cache, old client without meta + undecodable newer-envelope request pass through, linger and reopen). Unit tests: envelope, Level merge, queue drops.
- `tests/hub/examples/hub_loopback.rs`: loopback benchmark direct vs hub (in-process hub on a temp socket), alternating rounds, medians. `cargo run --release -p hub --example hub_loopback -- usb:c0de:cafe:<serial> 3 16 3`.
- resource-claims was told where to hook hub-side InUse (`State::request`).

## Benchmark so far (omarchy-m1, PC loaded by other sessions' builds, load 8-14: noisy)

- Max-size messages (1024 B): donguru 204B319B5431 within ±2 % on all figures.
- 16 B messages, single runs: round trips −12 … −24 % (+40-75 µs per round trip), host→device msgs −15 … −20 %, device→host −0 … −5 %. In-process: +12 µs per round trip. Cause found: task hops (each a cross-thread wake); removed the device-task hop on both paths (tunnel → event loop directly, replies routed on the rx task). Medians of the after-change run: see `$CLAUDE_JOB_DIR/tmp/bench1.txt` of the old job if still there, else rerun at low load (`uptime`).
- Criterion (design): ≤ 10 % fewer round trips/s on USB FS, ≤ 5 % throughput. Max-size meets it; 16 B still to be confirmed at low load. If still over: next hop to cut is the tunnel task on the reply path (write to the socket from the router), or measure per-hop with `tokio-console`/perf.

## Next

1. Rerun the benchmark at low load (all three boards: donguru 204B319B5431, B135 344533413431, b200 18AC50BA11FFB7C1; 16 and max), put a table into `docs/transport/hub.md`.
2. Docs section "Device tunnels (SRV-29)" in `docs/transport/hub.md` (anchor `#device-tunnels-srv-29` is already linked): connecting (`hub_device`, CLT-39's `WW_HUB`), what the hub does (seq remap, streams, observe merge, introspection cache, queues, linger, lock file mode hub, take), limits (shared counted/reliable streams: acks/resends from the first client only; sinks one client; streams on trait paths not counted, events of unknown paths go to all clients; loopback echoes to the client that sent last; device keys usb/rtt/inproc), cost + numbers. Level advanced (page already is).
3. FEATURES.md SRV-29 → 🧪 or ✅ with pointer (hardware measured on omarchy-m1), entry line in `tests/features_page/entries.md`, `just features-page`. Root `CHANGELOG.md` [Unreleased] entry. Finish wire_weaver_client CHANGELOG (see above), tests/hub needs none.
4. `just test`, `just lint` (fmt, `cargo sort -w -g`, clippy -D warnings, typos), ask before `just build-docs`.
5. `git rm HANDOFF.md`, commit, `tpm work ready --note "..."`, tell parent ww-hub it's ready to cherry-pick.

## Gotchas

- The PC is shared: use `CARGO_BUILD_JOBS=4 NEXTEST_TEST_THREADS=4`; benchmark numbers swing ±30 % with load.
- `ww_hub` tags: plain requests use `direct_tag` (bit 63); purpose tags (stream open, introspect) are counter tags in `State::pending`.
- Client introspection is requested with seq 0 and comes as seq-0 stream data at path `[]`: the hub treats root-path events while `Introspect::Fetching` as introspection.
Gotchas: interprocess 2.4 needs tokio >= 1.54 (workspace pinned). macOS can't chmod a socket file (listen falls back, the 0700 dir guards). Task texts that name other ids make tpm move those claims to the spawned session (resource-claims holds SRV-29 and CLT-41 by text only; told it).

# Handoff: session resource-claims (SRV-34)

## Done (part A)

- Typed API metadata, per the person's note of 21 Oct 2026 (replaces the planned `#[claimable]` marker): `ww_self::Meta { claim: Option<ClaimMode>, shares: Vec<Shares>, labels: Vec<String> }` on `ApiItem::meta` and `ApiLevel::meta`.
- Parser: `wire_weaver_core/src/transform/meta.rs` (`#[claim(exclusive|shared|observe)]`, `#[shares(pins, peripherals, index)]` with pin ranges, `#[label(..)]`), placement rules and errors; tests `meta_in_the_model`, `meta_misplaced_or_malformed`, `pin_ranges_expand`.
- `ApiItemOwned::claim_mode(&bundle)` / `label(index)`, `ClaimMode::name`; `ww api tree` shows the attributes; rustdoc notes; `ww api check` reports changes as compatible.
- Stdlib IO traits `#[claim(exclusive)]`, snapshots saved with `--force` (crates never published, versions stay 0.1.0).
- Docs `docs/api/metadata.md` (advanced), `docs/evolution/rules.md` "Metadata attributes"; FEATURES SRV-34 (in progress), API-56 care marker and API-57 device metadata (planned, also tpm P2401#21oct-2158, -2158b); design note in tpm ww-hub.md.

## Next (part B, after SRV-29 lands)

- `ww_hub_api` `Claims` group: `claim(device, path, purpose, ttl) -> Result<ClaimId, ClaimError { InUse { by: ClientMeta, since, purpose }, NotClaimable, Overlap }>`, `release`, `claims(device)`.
- Hub enforcement: other instances' calls and writes at/under a claimed path -> `Common(InUse)` (with label), not forwarded; reads/streams/observe open (taken for `observe`); `shared` semantics; overlap by path ancestry and `shares`; claims owned by (principal, instance id), not the self-reported ClientMeta instance id alone (parent, from the SRV-33 federation design: else a remote peer could join a local client's instance); today the principal is the local OS user (peer uid from the socket), keep a `principal` field so SRV-37 rights can fill it later; dropped on last connection close or ttl.
- Docs section in `docs/transport/hub.md` (hub vs device claims), tests (two clients, CAN bus path, InUse with metadata, stream keeps flowing, claimer killed -> claim gone).

## Gotchas

- Adding fields to the model writes their `None` flags: every signature/API hash changes once, so snapshots must be re-saved (`just save-snapshots --force`).
- `just build-docs` not run (asks the user first per AGENTS.md); links checked by hand.

# Handoff: session ww-hub

Plan: ~/git/tpm/prj/P2401-wireweaver/design/ww-hub.md (the design, read first). Umbrella FEATURES SRV-13.

Done (step 1, 21 Oct 2026): design written, FEATURES items SRV-13 (umbrella), SRV-28..33, CLT-38..40, TR-16, TOOL-17, TOOL-18 as planned items; tpm tasks P2401#21oct-2101 (API-21), #21oct-2102 (CLT-38), #21oct-2102b (TR-16 + SRV-28, kept by this session), #21oct-2102c (SRV-29), #21oct-2102d (CLT-39), #21oct-2102e (CLT-40 + SRV-30), #21oct-2103 (TOOL-17), #21oct-2103b (TOOL-18), #21oct-2103c (SRV-32), #21oct-2103d (SRV-33 design), #21oct-2103e (hardware check), #21oct-2039 rewritten as SRV-31. INT-12 was claimed by mistake (INT is introspection) and is unused.

Done (step 2): TR-16 committed (ww_local_socket, client transport, device medium, docs/transport/hub.md). Design extended per the person: direct by default / hub when it pays off (CLT-39), client metadata CLT-41 (session client-meta, omarchy-m1), per-resource claims SRV-34 (session resource-claims, omarchy-m1), polite handover CLT-43 (task P2401#21oct-2124). SRV-28 in progress: ww_hub_api (API v0, compiles), ww_hub (Bound::bind/run: hub.lock, socket, preamble dispatch, client tracking, idle exit, tests).

Next (this session): serve the hub API on Target::Hub connections (ww_device::Server over local_socket::device::connection_medium + ww_codegen server) once API-21 (session alloc-servers) is on the branch: cherry-pick or rebase onto it; then Registry from CLT-38's holders module (session device-holders, key_for) + nusb hotplug via wire_weaver_client::list_usb_devices; keep/shutdown handlers; drop the TODO(SRV-28) stub in serve_api and the dead_code allows. Then `ww hub serve` in the CLI (rest of TOOL-17 is its own task), docs, FEATURES SRV-28.

Decisions: one listener, WWHB preamble picks hub or device (frozen); lock files with std File::try_lock; readiness = the starter polls connect+preamble answer (no inherited pipe, works on Windows too; update the design when CLT-39 does it); open devices on demand + linger; newest hub wins via handover; federation via ssh-forwarded socket first.

Gotchas: interprocess 2.4 needs tokio >= 1.54 (workspace pinned). macOS can't chmod a socket file (listen falls back, the 0700 dir guards). Task texts that name other ids make tpm move those claims to the spawned session (resource-claims holds SRV-29 and CLT-41 by text only; told it).

# Handoff: session resource-claims (SRV-34)

Branch s/21oct2026-resource-claims, rebased onto hub-tunnels' s/21oct2026-hub-tunnels (f2460bb0, SRV-29 per-tunnel request path; fetched from omarchy-m1:/tmp/wt/hub-tunnels/wire_weaver). Part A tip before the rebase: 3f613c7f. Parent ww-hub OK'd part A; hub-tunnels said the request path is in, so part B started.

## Done

- Part A (commit "feat(api): typed metadata attributes"): `ww_self::Meta { claim, shares, labels }` on `ApiItem`/`ApiLevel`, `#[claim(exclusive|shared|observe)]`, `#[shares(pins, peripherals, index)]`, `#[label(..)]`; stdlib IO traits `#[claim(exclusive)]`; docs `docs/api/metadata.md`; FEATURES SRV-34 in progress, API-56/57 planned.
- Part B so far (wip commits):
  - `wire_weaver_client::decode_introspection(bytes)` (public): decode + inline snapshots + generics.
  - `ww_hub_api`: `Hub::claims: Claims` (`claim(key, path, purpose, ttl_s) -> Result<u32, ClaimError>`, `release(id)`, `claims(key)`), `ClaimKind`, `ClaimEntry`, `ClaimError { UnknownDevice, ApiUnknown, BadPath, NotClaimable, InUse{claim}, Overlap{claim} }`, `ReleaseError`, events `Claimed`/`Released`.
  - `ww_hub/src/claims.rs`: `Owner { principal: Principal::LocalUser, instance: Meta(id) | Connection(id) }` (parent: owner is (principal, instance), SRV-37 fills principals later), `resolve(bundle, path)` (root always exclusive; `claim_mode`, label, shares by index), `Claims` (claim with InUse/Overlap by path ancestry and shares, shared+shared same path allowed, same owner nests; release; drop_owner; expire by ttl; `refusing(key, owner, target, access)`), `trait_path` for global paths, `in_use_reply` (typed `Common(InUse{by})` via ww_self dynamic ser for methods returning `Result<_, E>` and property write errors, else `ErrorKind::UserStr("in use by ..")`). Unit tests pass.
  - `ww_hub/src/envelope.rs`: `access(request) -> Result<Option<(Access, Place, multi)>, ()>` (undecodable = Err, refused like Call anywhere when others hold claims), `introspection_bytes(events)`.
  - `ww_hub/src/device.rs`: `ToDevice::Bundle { reply }` (introspection fetched by the hub itself as requester `HUB_ITSELF` with seq 0, decoded lazily into `Task.bundle`), `Task::refused` called first in `Task::request`.
  - `ww_hub/src/lib.rs`: `owner_of`, `client_entry`, `Hub::claim/release/claims_on/claims_released`, `send_to_device` (take uses it), claims dropped in `client_left` when the instance's last connection goes; `BUNDLE_TIMEOUT` 5 s.
  - `ww_hub/src/api.rs`: `claims_*` handlers (Server has the connection `id`).
  - `tests/hub_api`: `#[claim] #[shares] #[label] ww_impl!(bus[]: Bus)` (send -> Result<u32, BusError>, rw speed with BusError, stream rx), `port: Port` (`#[claim(observe)]` trait, shares PA11 with bus[0]); `tests/hub` server got stubs (send echoes id, ping 1, valid indices 0..2; get/set speed still Unimplemented: make them real for the write test).

## Next

1. e2e tests in `tests/hub/src/lib.rs` (Bench: `bench.client_with(|c| c.client_meta(meta.clone()))` + `ww_hub::client::HubClient::config(|c| c.hub_api_at(bench.socket()).client_meta(meta.clone()))` so hub-API and tunnel share one instance id): A claims `bus[0]` ([3, 0]); B's `bus(0).send` -> `Err(BusError::Common(InUse{by}))` with A's program; B's speed write -> InUse; B's speed read and `rx` stream keep working (device sends with `stream_data_ser().bus(0)...` or similar); A's send works; claims(key) lists it; NotClaimable for `g`-like plain paths, BadPath, Overlap for `port` vs `bus[0]` (shares) and root; A killed (own runtime dropped, as in `killed_client_mid_stream`) -> Released event, B's send works; ttl expiry; observe-mode port: B's ping refused (UserStr since `ping` returns u8).
2. Docs: section in `docs/transport/hub.md` (level advanced): Claims API, owner = instance id + principal (share a `ClientMeta` between the hub client and device clients), what is refused per kind, typed InUse, global paths resolved via the device's API, undecodable requests refused while others hold claims, ttl/crash release; how hub claims and device claims combine (hub layer first, device layer after, neither takes the other; rePort b200_board Claim/Plan). Update `docs/api/metadata.md` Status note.
3. FEATURES SRV-34 -> done (plus entries.md line stays), `just features-page`; CHANGELOGs [Unreleased]: ww_hub, ww_hub_api, wire_weaver_client (decode_introspection), root CHANGELOG; ww-hub.md design note (tpm repo) short update.
4. `just test`, `just lint`, `cargo fmt`, `cargo sort -w -g`; squash the wip commits into "feat(hub): per-resource claims (SRV-34)"; `git rm HANDOFF.md`... but HANDOFF.md also holds ww-hub's note from the base branch: remove only this session's section, keep theirs; then `tpm work ready`.

## Gotchas

- `cargo ww` builds the CLI, which fails while ww_hub doesn't compile; `target/debug/ww api scaffold <api> --name Trait ...` works from the older build.
- Model fields write their `None` flags: snapshots re-saved with `--force` (never-published crates, versions stay).
- `just build-docs` not run (asks the user first per AGENTS.md).
- The PC is short on RAM at times: `CARGO_BUILD_JOBS=4`.

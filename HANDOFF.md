# Handoff: session ww-hub

Plan: ~/git/tpm/prj/P2401-wireweaver/design/ww-hub.md (the design, read first). Umbrella FEATURES SRV-13.

Done (step 1, 21 Oct 2026): design written, FEATURES items SRV-13 (umbrella), SRV-28..33, CLT-38..40, TR-16, TOOL-17, TOOL-18 as planned items; tpm tasks P2401#21oct-2101 (API-21), #21oct-2102 (CLT-38), #21oct-2102b (TR-16 + SRV-28, kept by this session), #21oct-2102c (SRV-29), #21oct-2102d (CLT-39), #21oct-2102e (CLT-40 + SRV-30), #21oct-2103 (TOOL-17), #21oct-2103b (TOOL-18), #21oct-2103c (SRV-32), #21oct-2103d (SRV-33 design), #21oct-2103e (hardware check), #21oct-2039 rewritten as SRV-31. INT-12 was claimed by mistake (INT is introspection) and is unused.

Done (step 2): TR-16 committed (ww_local_socket, client transport, device medium, docs/transport/hub.md). Design extended per the person: direct by default / hub when it pays off (CLT-39), client metadata CLT-41 (session client-meta, omarchy-m1), per-resource claims SRV-34 (session resource-claims, omarchy-m1), polite handover CLT-43 (task P2401#21oct-2124). SRV-28 in progress: ww_hub_api (API v0, compiles), ww_hub (Bound::bind/run: hub.lock, socket, preamble dispatch, client tracking, idle exit, tests).

Next (this session): serve the hub API on Target::Hub connections (ww_device::Server over local_socket::device::connection_medium + ww_codegen server) once API-21 (session alloc-servers) is on the branch: cherry-pick or rebase onto it; then Registry from CLT-38's holders module (session device-holders, key_for) + nusb hotplug via wire_weaver_client::list_usb_devices; keep/shutdown handlers; drop the TODO(SRV-28) stub in serve_api and the dead_code allows. Then `ww hub serve` in the CLI (rest of TOOL-17 is its own task), docs, FEATURES SRV-28.

Decisions: one listener, WWHB preamble picks hub or device (frozen); lock files with std File::try_lock; readiness = the starter polls connect+preamble answer (no inherited pipe, works on Windows too; update the design when CLT-39 does it); open devices on demand + linger; newest hub wins via handover; federation via ssh-forwarded socket first.

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
- Hub enforcement: other instances' calls and writes at/under a claimed path -> `Common(InUse)` (with label), not forwarded; reads/streams/observe open (taken for `observe`); `shared` semantics; overlap by path ancestry and `shares`; claims owned by instance id (CLT-41), dropped on last connection close or ttl.
- Docs section in `docs/transport/hub.md` (hub vs device claims), tests (two clients, CAN bus path, InUse with metadata, stream keeps flowing, claimer killed -> claim gone).

## Gotchas

- Adding fields to the model writes their `None` flags: every signature/API hash changes once, so snapshots must be re-saved (`just save-snapshots --force`).
- `just build-docs` not run (asks the user first per AGENTS.md); links checked by hand.

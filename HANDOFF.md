# Handoff: session ww-hub

Plan: ~/git/tpm/prj/P2401-wireweaver/design/ww-hub.md (read first). Umbrella FEATURES SRV-13. Tasks P2401#21oct-2101..2103e, #21oct-2122/2122b, #21oct-2124, #21oct-2138, #21oct-2039 (SRV-31).

Done on this branch (s/21oct2026-ww-hub):
- TR-16 local socket transport (ww_local_socket, wire_weaver_client feature local_socket, device medium, connection_medium + Ended).
- SRV-28 hub: ww_hub_api, ww_hub (Bound::bind/run, API served per connection via generated no_alloc=false server, devices + holders registry, events, keep/shutdown, HubClient), ww hub serve, main_if_hub. Tested on Linux; Windows/macOS compile+clippy only.
- Cherry-picked from children: CLT-38 holders (device-holders), CLT-41 client meta (client-meta, omarchy-m1), API-21 alloc servers (alloc-servers). Fix commit: holders use ww_local_socket::runtime_dir (feature `socket` split so the dir is cheap).
- FEATURES items for the whole plan; design extended by the person's notes (direct by default + hub when it pays off, CLT-43 handover, CLT-44 live link handover, SRV-34 claims, CLT-41 meta).

Running children: hub-cli (TOOL-17, sonnet, omarchy-m1), hub-federation (SRV-33 design, top, omarchy-m1), hub-tunnels (SRV-29, top, omarchy-m1), hub-routing (CLT-39, opus, omarchy-m1), resource-claims (SRV-34, parked for part B until hub-tunnels says so). Both hub-* sessions start by resetting onto this branch (git fetch gpd-omarchy:/tmp/wt/ww-hub/wire_weaver s/21oct2026-ww-hub).

Next for this session: nothing big of its own; answer children, take their work up when ready (cherry-pick or they land after this branch). Then the remaining tasks per the design's order: CLT-40+SRV-30 (top), SRV-31 clock sync (opus, sync-converge landed), TOOL-17 (sonnet), TOOL-18 (sonnet), CLT-43 (opus), CLT-44 (top), SRV-32 (opus), SRV-33 design (top), hardware check (sonnet, omarchy-m1). Spawn on omarchy-m1 by default (person's wish), from this branch via fetch + reset until it lands.

Decisions: frozen WWHB preamble; std File::try_lock lock files; readiness = starter polls connect + preamble answer; hub API served with ww_device::Server per connection; device tunnels answer UnknownDevice until SRV-29; handover/take answer Unimplemented until SRV-30/SRV-29.

Gotchas: interprocess 2.4 needs tokio >= 1.54 (workspace pinned). macOS can't chmod a socket file. ww_device::PeerMeta keeps only program/user/host (24 B each), kind, pid, instance id: label/app_version/purpose aren't visible to the hub yet (SRV-29 terminates tunnel links itself and can keep the full ClientMeta). Task texts naming other ids make tpm move those claims to the spawned session. wire_weaver_client --no-default-features clippy has a pre-existing dead-code error in client/observe.rs (not from this branch).

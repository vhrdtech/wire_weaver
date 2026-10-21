# Handoff: session ww-hub

Plan: ~/git/tpm/prj/P2401-wireweaver/design/ww-hub.md (read first). Umbrella FEATURES SRV-13. Tasks P2401#21oct-2101..2103e, #21oct-2122/2122b, #21oct-2124, #21oct-2138, #21oct-2039 (SRV-31).

Done on this branch (s/21oct2026-ww-hub):
- TR-16 local socket transport (ww_local_socket, wire_weaver_client feature local_socket, device medium, connection_medium + Ended).
- SRV-28 hub: ww_hub_api, ww_hub (Bound::bind/run, API served per connection via generated no_alloc=false server, devices + holders registry, events, keep/shutdown, HubClient), ww hub serve, main_if_hub. Tested on Linux; Windows/macOS compile+clippy only.
- Cherry-picked from children: CLT-38 holders (device-holders), CLT-41 client meta (client-meta, omarchy-m1), API-21 alloc servers (alloc-servers). Fix commit: holders use ww_local_socket::runtime_dir (feature `socket` split so the dir is cheap).
- FEATURES items for the whole plan; design extended by the person's notes (direct by default + hub when it pays off, CLT-43 handover, CLT-44 live link handover, SRV-34 claims, CLT-41 meta).

Taken up (cherry-picked) since: CLT-39 hub routing (hub-routing), TOOL-17 ww hub status/stop/who/keep (hub-cli; CLI's own hub_running dropped for wire_weaver_client::hub::hub_running), SRV-33 federation FEATURES items (hub-federation; design prj/P2401-wireweaver/design/ww-hub-federation.md waits for the person's review, pieces SRV-36..40, API-58; low findings CLT-45, TOOL-19). Those three sessions are ready; the person closes them.

Taken up 22 Oct: SRV-29 tunnels (hub-tunnels; benchmark: throughput as direct, 16 B USB FS round trips -15..-23 % idle, -3..-13 % loaded, from two extra process wakes; busy-poll / shared memory left to the person), SRV-34 claims part A+B (resource-claims; claim owner = (principal, instance id)). Fix: the hub opens its devices with HubMode::Direct. Branch rebased onto master (has sync-converge STD-49/TOOL-16) with tpm work rebase. hub-tunnels, resource-claims ready; the person closes them.

Running children (spawned 22 Oct): see `tpm work ls`; hub-reconnect (CLT-40+SRV-30, top), hub-clock-sync (SRV-31, opus, omarchy-m1), hub-python (TOOL-18, sonnet), hub-exclusive (SRV-32, opus, omarchy-m1). Each starts from this branch's HEAD.

Next for this session: take their work up as they get ready, then CLT-43 (after CLT-40), CLT-44 (top, after CLT-43), hardware check P2401#21oct-2103e (sonnet, omarchy-m1) last. SRV-33 federation pieces wait for the person's review.

Decisions: frozen WWHB preamble; std File::try_lock lock files; readiness = starter polls connect + preamble answer; hub API served with ww_device::Server per connection; handover answers Unimplemented until SRV-30.

Gotchas: interprocess 2.4 needs tokio >= 1.54 (workspace pinned). macOS can't chmod a socket file. ww_device::PeerMeta keeps only program/user/host (24 B each), kind, pid, instance id: label/app_version/purpose aren't visible to the hub yet (SRV-29 terminates tunnel links itself and can keep the full ClientMeta). Task texts naming other ids make tpm move those claims to the spawned session. wire_weaver_client --no-default-features clippy has a pre-existing dead-code error in client/observe.rs (not from this branch).

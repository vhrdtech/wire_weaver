# Handoff: session ww-hub

Plan: ~/git/tpm/prj/P2401-wireweaver/design/ww-hub.md (read first). Umbrella FEATURES SRV-13. Tasks P2401#21oct-2101..2103e, #21oct-2122/2122b, #21oct-2124, #21oct-2138, #21oct-2039 (SRV-31).

Done on this branch (s/21oct2026-ww-hub):
- TR-16 local socket transport (ww_local_socket, wire_weaver_client feature local_socket, device medium, connection_medium + Ended).
- SRV-28 hub: ww_hub_api, ww_hub (Bound::bind/run, API served per connection via generated no_alloc=false server, devices + holders registry, events, keep/shutdown, HubClient), ww hub serve, main_if_hub. Tested on Linux; Windows/macOS compile+clippy only.
- Cherry-picked from children: CLT-38 holders (device-holders), CLT-41 client meta (client-meta, omarchy-m1), API-21 alloc servers (alloc-servers). Fix commit: holders use ww_local_socket::runtime_dir (feature `socket` split so the dir is cheap).
- FEATURES items for the whole plan; design extended by the person's notes (direct by default + hub when it pays off, CLT-43 handover, CLT-44 live link handover, SRV-34 claims, CLT-41 meta).

Taken up (cherry-picked) since: CLT-39 hub routing (hub-routing), TOOL-17 ww hub status/stop/who/keep (hub-cli; CLI's own hub_running dropped for wire_weaver_client::hub::hub_running), SRV-33 federation FEATURES items (hub-federation; design prj/P2401-wireweaver/design/ww-hub-federation.md waits for the person's review, pieces SRV-36..40, API-58; low findings CLT-45, TOOL-19). Those three sessions are ready; the person closes them.

Taken up 22 Oct: SRV-29 tunnels (hub-tunnels; benchmark: throughput as direct, 16 B USB FS round trips -15..-23 % idle, -3..-13 % loaded, from two extra process wakes; busy-poll / shared memory left to the person), SRV-34 claims part A+B (resource-claims; claim owner = (principal, instance id)). Fix: the hub opens its devices with HubMode::Direct. Branch rebased onto master (has sync-converge STD-49/TOOL-16) with tpm work rebase. hub-tunnels, resource-claims ready; the person closes them.

Taken up 22 Oct ~02:00: CLT-40 + SRV-30 (hub-reconnect), SRV-32 (hub-exclusive), TOOL-18 (hub-python), SRV-31 (hub-clock-sync, incl. bedrock-steer's PeerMeta flash fix). Integration commit a1fae657: steering pauses under a root claim (Hub::claims_changed pokes Steering), connect_blocking IPC on the runtime handle, Lent numbers pinned by a test. Gate: just test (1977 + Python), just lint, just build-docs clean. Merged sessions closed (branches kept); stray local branch tmp-reconnect (= hub-reconnect's tip) left, the guard won't let me delete it.

Taken up: CLT-46 auto clock sync (auto-clock-sync, closed); fix: no unit Route without the hub feature (clippy -p wire_weaver_client). rustdoc without --all-features fails on master's ww_defmt_logger/ww_device/ww_stream links (just lint uses --all-features: clean).

Taken up: CLT-43 polite handover (hub-handover, closed); gate incl. build-docs clean at b81d53c5.

Running: hub-live-handover (CLT-44, top, omarchy-m1; simulated parts first, real-board measurement when the bench hub is back; plan prj/P2401-wireweaver/design/ww-hub-live-handover.md: no fd passing (nusb 0.2 can't), holder releases at alt 0, hub opens via sysfs + claims, LinkSnapshot over the hub API adopt_prepare/commit; I asked for usbmon proof on a board, fallback to CLT-43 if the interface is grabbed in the gap, bounded snapshot validation, FEATURES CLT-44 text updated), hub-clock-sync (parked on omarchy-m1 for the hardware check: the bench USB hub dropped all boards, the person replugs it; its child bedrock-steer does the firmware side in embedded_bedrock).

Taken up 22 Oct 03:55: CLT-44 simulated parts (hub-live-handover up to 66cd4091: framer carry-over, usb_sim, LinkSnapshot pause/relink/adopt, hub Adopt API, failure tests, Linux nusb hooks, docs). Gate green after the cherry-pick (just test 2000, just lint). hub-live-handover stays parked for its real-board check (usbmon proof on omarchy-m1); take up its later commits when it is ready.

22 Oct ~11:00: the person decided R5 stays fixed and no federation now (SRV-36..40, API-58 stay in Later; review rules folded into their task texts; scope noted in the federation design). Done here: CLT-45 one lock file per device key (holders::file_name, _xx escaping, SHA-256 for long names, key checked on read), TOOL-19 escape device and hub strings in ww output (wire_weaver_cli/src/untrusted.rs, by a subagent). Taken up from hub-clock-sync (ready, checked on Donguru + B135): e2221cdc (SRV-31 and STD-49 done with the hardware numbers, FEATURES merged by hand) and bf595b60. Gate: just lint green; just test green except hub_reconnect live_handover a_holder_hands_its_live_link_to_the_hub, flaky at load avg ~25 (9 calls failed after 1 s), passed to hub-live-handover to find the cause.

Next: take up hub-live-handover when ready (board check of CLT-44 + the flaky test fix), rerun the gate, then tpm work ready. SRV-29 and SRV-32 hardware items: hub-clock-sync only checked SRV-31; check whether SRV-29/SRV-32 are still 🧪 and say so in the report.
 hardware check P2401#21oct-2103e on omarchy-m1 once the boards are back (SRV-29/31/32 🧪 items); flaky io_timebase test filed as P2401#22oct-0211 (sonnet). SRV-33 federation pieces (SRV-36..40, API-58) wait for the person's review (morning of 22 Oct).

Decisions: frozen WWHB preamble; std File::try_lock lock files; readiness = starter polls connect + preamble answer; hub API served with ww_device::Server per connection; handover answers Unimplemented until SRV-30.

Gotchas: interprocess 2.4 needs tokio >= 1.54 (workspace pinned). macOS can't chmod a socket file. ww_device::PeerMeta keeps only program/user/host (24 B each), kind, pid, instance id: label/app_version/purpose aren't visible to the hub yet (SRV-29 terminates tunnel links itself and can keep the full ClientMeta). Task texts naming other ids make tpm move those claims to the spawned session. wire_weaver_client --no-default-features clippy has a pre-existing dead-code error in client/observe.rs (not from this branch).

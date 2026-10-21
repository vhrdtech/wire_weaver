# Handoff: clock-sync (P2528 pieces 1 and 2)

Design: ~/git/tpm/maintain/P2528-embedded-bedrock/design/clock-sync.md ("Rules" and "As built").

## Done (wire_weaver, this branch)
- STD-49: ww_timebase sources model (UTC, line > host > holdover > free), host_sync group (steer -> SyncSource, shared_now), SyncSource, SyncStatus.source/error, TimeMapPoint.source, SyncState::Conflict. Snapshots saved with --force (0.1.0 never published).
- ww_clock_sync crate: Estimator, DeviceTime, HostClock, ClockSync. Tests: simulated clocks + tests/io_timebase e2e.
- STD-50: ww_indication alignment rule (docs only; set_mode_with_epoch dropped by decision).
- TOOL-16: ww sync. Skill tree filed: tpm P2618#21oct-1707.
- EVO-10 filed: checker calls an appended Unsized-enum variant breaking.

## Next
- embedded_bedrock (worktree ../embedded_bedrock, same branch): bedrock_sync (DEV-13) with oscillator backend trait, bedrock_led time-based (DEV-14).
- Gates: just test / just lint in both repos; HANDOFF.md git rm; tpm work ready; say parent; spawn Donguru (P2534#21oct-1639) and rePort (P2424#21oct-1639) on sonnet. B135 (P2404#21oct-1640, #21oct-1657) stays filed.

## Gotchas
- Firmware serving Clock/Indication (Donguru, rePort) won't build against this until they add omit = ["clock.host_sync"] or the handlers, and source/error fields in SyncStatus/TimeMapPoint literals.
- Donguru's ww_numeric is u32-only: SteerPoint.shared_us is a plain u64 for that reason.

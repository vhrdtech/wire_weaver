# HANDOFF: medium-residual (P2401#22oct-1442)

## Done

- `wire_weaver_client::adopt`: `LinkSnapshotOwned` split into `HandoverOwned { client_version, key, link: LinkResidualOwned, medium: MediumResidualOwned }`. The link residual is medium independent (token, DeviceInfo, seqs, in flight, full_only, streams, observed, ping timers) and checked on its own; the medium residual is `kind` + opaque bytes, decoded only by that medium (`usb::UsbResidualOwned`, `usb_sim::UsbSimResidualOwned`, both carrying `framing::FramerCarryOwned`).
- `event_loop/medium.rs`: one transport slot, `MediumTx` / `MediumRx`, an enum over every medium of the crate (`media!` macro) plus `Other(Box<dyn ..>)` for test transports (`other_medium!`). Per-medium steps stay on `Transport` (pause, let_go, take_back, switch, claims), dispatched by variant (`with_medium!`). `medium::relink` swaps the slot from any medium to any other and calls `TxCore::relink`. `Live<D, H>` is gone; `Transport::switch` no longer takes a tunnel.
- Hub side (`ww_hub` adopt.rs, device.rs) uses `HandoverOwned`; scheme check by `MediumKindOwned::scheme()`. USB adopt checks interface/endpoints fit.
- Docs: hub.md "The residual: link and medium"; FEATURES CLT-44 text; CHANGELOGs (wire_weaver_client breaking + feature, ww_hub fix).

## Measured (gpd-omarchy, debug, usb_sim, alternating A/B at load ~30)

- handover median: base 6.88 ms vs new 6.72 ms (12 runs each).

## Next

- just test + just lint, commit, rePort live_bench on omarchy-m1 (scratch folder with sibling chunky_mcu_updater), report, then `git rm HANDOFF.md`, tpm work ready, tpm work say parent.

## Gotchas

- A baseline checkout needs `chunky_mcu_updater` as its sibling (wire_weaver_cli path dep), and not inside the session folder (lockfile collision).
- `wire_weaver_client` unit tests behind `local_socket` only run with the workspace's unified features (`cargo nextest run --workspace -E ...`), not with `-p wire_weaver_client`.

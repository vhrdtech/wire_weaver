# HANDOFF: dfu-scenarios

Task: ww_dfu docs page with one section per scenario + one e2e test per scenario with golden transcripts.

## Done
- `wire_weaver_client::in_process::usb_device` (simulated USB, real framing/packet sizes), committed.
- `tests/dfu_{app,lean_app,updater}_api`: three API roots (app with all traits, lean app node+boot, updater).
- `tests/dfu_scenarios`: plan.rs (toy plan format + planners), flash.rs (NOR flash, bootloader, journal, cuts),
  node.rs (trait logic), firmware.rs (generated servers), device.rs (device thread, Link enum, reboots),
  host.rs (clients, connect!, send_steps!), record.rs (taps, readable + wire transcript, golden check),
  scenarios.rs (2C-APP-STEP so far). Golden files in tests/dfu_scenarios/transcripts/, regenerate with
  `DFU_TRANSCRIPTS=update cargo nextest run -p dfu_scenarios`.

## Next
- Remaining 7 scenarios (see plan below), just recipe `dfu-transcripts`, docs page, changelogs, FEATURES STD-12.

## Scenario plan (code, word name, link, twist)
- 2C-APP-STEP Swap Shuttle, USB FS: resend of a step, trial + confirm
- 2C-APP-BULK Swap Freight, UDP: host restarts mid-stream, resumes
- 2R-APP-STEP Flip Shuttle, WS: full-image plan, trial + confirm
- 2R-APP-BULK Flip Freight, USB HS: trial not confirmed -> revert
- 1S-APP-STEP Parcel Shuttle, USB FS: power cut while bootloader applies, journal resume
- 1S-APP-BULK Parcel Freight, UDP: app keeps answering mid-download
- 1L-UPD-STEP Pit-stop Shuttle, USB FS: reboot into updater, power cut mid-download, resume
- 1L-UPD-BULK Pit-stop Freight, WS: plain

## Gotchas
- Link setup bytes carry crate versions: left out of transcripts on purpose (release bumps would break goldens).
- WS host frames have random mask keys: recorded unmasked with zeroed key.

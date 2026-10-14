# ww_dfu

Firmware update API, shared by every device that updates over WireWeaver. The full description lives in the crate
docs (`cargo doc -p ww_dfu --open`); in short:

- **Traits, each node picks:** `UpdateNode` (who it is, what it offers; every node), `BootControl` (boot state,
  confirm, revert, reboot into the app or the updater), `Inventory` (the manifest, paged), `Stage` (receive a plan
  step by step; session control), `StageBulk` (receive a whole plan as one stream).
- **Three update modes:** A/B applied live by the app; in place from a plan the app stores, applied by the
  bootloader after a reset (no transport in the bootloader); in place applied live by a bootloader or recovery app
  with a transport.
- **Two ways a plan travels:** one call per step (fast links), or the whole plan in one go on a credit sink
  ([flow control](../api/streams.md#credit-based-flow-control-for-sinks): never more segments than the device has
  room for, lost ones resent) with one acknowledgement at the end (slow and remote links).
- **The device trusts nothing it receives:** bounds, hashes, version floor and signature are checked on the device.

The plan, manifest and descriptor formats are defined in octets by the chunky protocol spec
(`chunky_mcu_updater`, `SPEC.md`); signed records travel as raw bytes. `tests/dfu` drives every trait end to end
against a fake device.

## Unreleased

### ⚠️ Breaking

- The `FirmwareUpdate` stub is replaced by the update API (STD-12): traits `UpdateNode`, `BootControl`, `Inventory`,
  `Stage` and `StageBulk`, each node implementing the ones it needs; three update modes (A/B applied live, in place
  from a stored plan, in place applied live by an updater) and two ways a plan travels (step by step, or in one go
  with acknowledgements only when needed). Signed records of the chunky protocol travel as raw octets. Crate docs
  describe the modes, resuming and what the device checks.

# mount_module

A mount slot served in the device's own process (SRV-81): the `mounts_api` TV serves its hub slot from a module through `ww_device::mount::ModuleSlot`, under `tests_common::start_device_async`. Covers calls, streams, presence following load and unload, a reload relearned for that slot only, and modules that misbehave or fail.

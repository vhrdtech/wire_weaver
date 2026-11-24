# mount_module

A mount slot served in the device's own process (SRV-81): the `mounts_api` TV, an async `no_alloc` server, serves its hub slot from a module through `ww_device::mount::ModuleSlot`, under `tests_common::start_device_async`. Covers calls, streams, presence following load and unload, both slots' borrowed presences observed at once (SRV-83), a reload relearned for that slot only, and modules that misbehave or fail.

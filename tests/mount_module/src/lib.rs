//! A mount slot served in the device's own process (SRV-81): the TV of `mounts_api` serves its hub slot from a
//! module through `ww_device::mount::ModuleSlot`, with no link to another process. The modules are servers linked
//! into the test (the hub of `mount_backend_api`, a second TV) and one that misbehaves.

pub mod modules;
pub mod tv;

#[cfg(test)]
mod tests;

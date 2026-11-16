//! Root for the global trait path dispatch bench (API-20, `examples_mcu/mcu_qemu` `qemu-global-paths`, R10): `alpha`
//! is a local, minimal trait (index 0); `clock` is `ww_timebase::Clock` (index 1), kept because it is a framework
//! trait with a real `ww_global` compact id (24) the bench needs to exercise a `GlobalCompact` match. A
//! framework-only trait of its own has no id until SRV-61 gives non-stdlib traits one too.
#![no_std]

use ww_api::prelude::*;

#[ww_api_root]
pub trait PathBenchDevice {
    ww_impl!(alpha: Alpha);
    ww_impl!(clock: ww_timebase::Clock);
}

#[ww_trait]
pub trait Alpha {
    fn ping() -> bool;
}

//! Mount slots (SRV-76): a TV whose hub and extensions are APIs of other processes, known only at run time.
//! `mount!(hub: hub_api)` is one slot, `mounts!(ext)` an array of slots keyed by name.
#![cfg_attr(not(feature = "std"), no_std)]

use ww_api::prelude::*;

#[ww_api_root]
#[effect(edit)]
trait Tv {
    /// The home hub, when its process runs.
    mount!(hub: hub_api);
    /// Extensions by name: any local program can be mounted.
    mounts!(ext);
    /// Mutes the sound.
    fn mute();
}

/// The doc trait shows the slots as `Mount` and the names of `ext` as a key companion: checked by the compiler.
#[expect(
    dead_code,
    reason = "compile-time check of the doc trait's signature, never called"
)]
mod doc_trait {
    use super::*;
    use ww_api::api_doc::{Keys, Mount};
    use ww_api::mount::MountName;
    use ww_keyed::KeyEvent;

    fn slot(_: fn() -> Mount) {}
    fn slots(_: fn(UNib32) -> Mount) {}
    fn keys(_: fn() -> Keys<KeyEvent<UNib32, MountName<'static>>>) {}

    fn tv<T: Tv>() {
        slot(T::hub);
        slots(T::ext);
        keys(T::ext_keys);
    }
}

//! Types of mount slots (SRV-76): `mount!(hub)` declares one slot whose API is known only at run time,
//! `mounts!(ext)` an array of them keyed by name.
//!
//! A slot is a fixed level of two items: `presence` (id 0), a read-only observed [MountPresence], and `api` (id 1),
//! the root of whatever API is mounted there. A request under `api` reaches the mounted backend with the slot's path
//! taken off. The parent's API hash covers only the declaration, never the mounted API: that one has its own
//! introspection, hash and version, in [MountPresence::Up].

use shrink_wrap::prelude::*;
use ww_version::{ApiHashPair, FullVersion, WireVersion};
#[cfg(feature = "std")]
use ww_version::{ApiHashPairOwned, FullVersionOwned};

/// Whether a mount slot has a backend, and which API it serves.
#[derive_shrink_wrap(owned(feature = "std"), derive(Clone, Debug), ww_repr = nib)]
pub enum MountPresence<'i> {
    /// Nothing is mounted: a request under the slot's `api` fails at once with `Unavailable`.
    Absent,
    /// A backend is mounted and serves the API `api`.
    Up {
        /// Crate and version of the mounted API.
        api: FullVersion<'i>,
        /// Hashes of the mounted API's introspection data: a client relearns the slot's API only when they change.
        hash: ApiHashPair<'i>,
        /// Wire version of the mounted API crate, `None` when it declares none.
        wire: Option<WireVersion>,
    },
}

/// Name of a slot of `mounts!(name)`, the key a client addresses the slot by: at most 64 bytes.
#[derive_shrink_wrap(
    owned(feature = "std"),
    derive(Clone, Debug, PartialEq, Eq),
    derive_borrowed(Copy)
)]
pub struct MountName<'i>(pub MaxLen<&'i str, 64>);

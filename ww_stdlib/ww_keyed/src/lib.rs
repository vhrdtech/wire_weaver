#![cfg_attr(not(feature = "std"), no_std)]
//! Wire types of keyed resource arrays (API-67).
//!
//! `keyed!(flow by FlowKey)` next to an array `flow[]` lets a client address its elements by a key instead of an
//! index. The device announces each element on the companion's `keys` stream with a [KeyEvent]; the generated
//! code names the index and key types, so this crate only fixes their wire form. See `docs/api/arrays.md`.

use shrink_wrap::prelude::*;

/// An announcement on the `keys` stream of a `keyed!` array: an element appeared or went away.
///
/// `I` is the array's index type (`UNib32` for an array declared as `name[]`), `K` the key type of `keyed!`.
#[derive_shrink_wrap(
    borrowed,
    owned(feature = "std"),
    cfg_attr(feature = "defmt", derive(defmt::Format)),
    derive(Clone, Debug, PartialEq),
    ww_repr = nib
)]
pub enum KeyEvent<I, K> {
    /// An element appeared at `index` under `key`; sent once per element.
    Added { index: I, key: K },
    /// The element at `index` is gone; the device may give the index to another key after this.
    Removed { index: I },
}

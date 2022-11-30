#![cfg_attr(feature = "no_std", no_std)]

//! Data structures describing root functionality of xPI - x Programming Interface.
//! Every field and enum item is excessively documented in this mod, while concrete implementations
//! simply point here.
//! Types is kept highly generic, to be able to create owned and borrowed implementations downstream
//! while also keeping them all coherent.
pub mod event;
pub mod event_kind;
pub mod node_set;
pub mod priority;
pub mod resource_set;

pub mod error;

pub mod xwfd;

#[cfg(not(feature = "no_std"))]
pub mod owned;

pub mod reply_size_hint;

pub use reply_size_hint::ReplySizeHint;
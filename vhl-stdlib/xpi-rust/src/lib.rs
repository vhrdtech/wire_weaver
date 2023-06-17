#![cfg_attr(feature = "no_std", no_std)]

//! Data structures describing root functionality of xPI - x Programming Interface.

// pub mod event;
// pub mod event_kind;
// pub mod node_set;
// pub mod priority;
// pub mod resource_set;

pub mod error;

// pub mod xwfd;

#[cfg(not(feature = "no_std"))]
pub mod node_owned;

// pub mod reply_size_hint;
// pub use reply_size_hint::ReplySizeHint;

pub mod client_server;

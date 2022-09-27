//! Data structures describing root functionality of xPI - x Programming Interface.
//! Every field and enum item is excessively documented in this mod, while concrete implementations
//! simply point here.
//! Types is kept highly generic, to be able to create owned and borrowed implementations downstream
//! while also keeping them all coherent.
pub mod event;
pub mod request;
pub mod reply;
pub mod broadcast;
pub mod addressing;
pub mod priority;

#[cfg(feature = "std")]
pub mod owned;
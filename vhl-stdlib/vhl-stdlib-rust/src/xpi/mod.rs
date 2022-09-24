//! Data structures describing root functionality of xPI - x Programming Interface.
//! Every field and enum item is excessively documented in this mod, while concrete implementations
//! simply point here.
//! Types is kept highly generic, to be able to use owned and borrowed arrays downstream.
//! Everything is kept in once place in this mod, so that all implementations are coherent.
pub mod event;
pub mod request;
pub mod reply;
pub mod broadcast;
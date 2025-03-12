#![warn(clippy::all, rust_2018_idioms)]

mod app;
pub mod context;
mod debug_view;
mod tab;
mod tab_kind;
mod tiles_demo;
mod util;

pub use app::WireWeaverToolApp;

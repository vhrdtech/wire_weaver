//! The bodies of the ww_tape fuzz targets, one function per target: `fuzz/src/tape_<name>.rs` calls it with
//! libFuzzer's input.
#![expect(
    clippy::collapsible_if,
    clippy::chunks_exact_to_as_chunks,
    reason = "bodies moved from fuzz/ as they were; the next commit takes clippy's forms"
)]

mod chunk;
mod index;
mod payloads;
mod reader;
mod samples;
mod series;
mod summary;
mod times;

pub use chunk::chunk;
pub use index::index;
pub use payloads::payloads;
pub use reader::reader;
pub use samples::samples;
pub use series::series;
pub use summary::summary;
pub use times::times;

//! The bodies of the ww_tape fuzz targets, one function per target: `fuzz/src/tape_<name>.rs` calls it with
//! libFuzzer's input.

mod chunk;
mod hostile;
mod index;
mod payloads;
mod reader;
mod samples;
mod series;
mod summary;
mod times;

pub use chunk::chunk;
pub use hostile::{
    chunk_hostile, index_hostile, payloads_hostile, reader_hostile, records_hostile,
    samples_hostile, series_hostile, summary_hostile, times_hostile,
};
pub use index::index;
pub use payloads::payloads;
pub use reader::reader;
pub use samples::samples;
pub use series::series;
pub use summary::summary;
pub use times::times;

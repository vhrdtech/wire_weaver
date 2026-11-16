//! The bodies of the ww_tape fuzz targets, one function per target: `fuzz/src/tape_<name>.rs` calls it with
//! libFuzzer's input.

mod chunk;
mod hostile;
mod index;
mod payloads;
mod reader;
mod resume;
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
pub use resume::resume;
pub use samples::samples;
pub use series::series;
pub use summary::summary;
pub use times::times;

/// A fuzz target's body.
pub type Target = fn(&[u8]);

/// Every ww_tape fuzz target by its name in `fuzz/Cargo.toml`.
pub const ALL: [(&str, Target); 18] = [
    ("tape-chunk", chunk),
    ("tape-index", index),
    ("tape-payloads", payloads),
    ("tape-reader", reader),
    ("tape-samples", samples),
    ("tape-series", series),
    ("tape-summary", summary),
    ("tape-times", times),
    ("tape-chunk-hostile", chunk_hostile),
    ("tape-index-hostile", index_hostile),
    ("tape-payloads-hostile", payloads_hostile),
    ("tape-reader-hostile", reader_hostile),
    ("tape-records-hostile", records_hostile),
    ("tape-samples-hostile", samples_hostile),
    ("tape-series-hostile", series_hostile),
    ("tape-summary-hostile", summary_hostile),
    ("tape-times-hostile", times_hostile),
    ("tape-resume", resume),
];

/// The target named `name`.
pub fn by_name(name: &str) -> Option<Target> {
    ALL.iter().find(|(n, _)| *n == name).map(|(_, t)| *t)
}

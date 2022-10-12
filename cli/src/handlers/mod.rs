pub mod repl_xpi;
pub mod generate;
pub mod dev;

pub mod prelude {
    pub use anyhow::{anyhow, Context, Result};
    pub use crate::util;
    pub use std::path::PathBuf;
    pub use ast::{SpanOrigin, SourceOrigin};
}
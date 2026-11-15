//! The hostile block builders and the fuzz targets of ww_tape_fuzz, each case under a cap on single allocations.

mod builders;
mod limits;

use ww_tape_fuzz::case::Capped;

#[global_allocator]
static ALLOCATOR: Capped = Capped;

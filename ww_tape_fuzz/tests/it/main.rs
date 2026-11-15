//! The hostile block builders, the seed files and the case runner of ww_tape_fuzz, each case under a cap on single
//! allocations.

mod builders;
mod limits;
mod seeds;

use ww_tape_fuzz::case::Capped;

#[global_allocator]
static ALLOCATOR: Capped = Capped;

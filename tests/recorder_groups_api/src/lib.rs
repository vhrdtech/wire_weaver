//! Test API root wrapping `ww_recorder::Recorder`, used by `tests/recorder_groups` to check that a small firmware
//! can leave every optional group out with `omit_if` (STD-34, see `docs/std_library/ww_recorder.md`).
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;

#[ww_api_root]
pub trait RecorderGroupsTest {
    ww_impl!(recorder: ww_recorder::Recorder);
}

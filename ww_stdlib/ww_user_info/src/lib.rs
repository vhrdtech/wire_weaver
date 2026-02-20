// #![cfg_attr(not(feature = "std"), no_std)]
#![no_std]

use wire_weaver::prelude::*;

/// User configurable device name and notes
#[ww_trait]
pub trait UserInfo {
    property!(rw name: str);
    property!(rw notes: str);
}
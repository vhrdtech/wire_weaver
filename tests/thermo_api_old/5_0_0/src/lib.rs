//! `thermo_api` 5.0.0: lines 5 and 6 of the multi-line bench (EVO-28, EVO-42) at their first revisions, [wire_v1] at
//! w5.0 before `calibrate` (w5.1) and `uptime` (w5.2), [wire_v2] at w6.0 before `self_test` (w6.1). Line 7 came with
//! 6.0.0. Firmware in the field may still run it; the bench's host has no client of it, it talks to it with the
//! clients of w5.2 and w6.1.
#![cfg_attr(not(feature = "std"), no_std)]

pub mod common;
pub mod wire_v1;
pub mod wire_v2;

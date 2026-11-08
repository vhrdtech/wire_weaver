//! A few values of a power converter observed at its switching rate, for the `examples_mcu` high-rate observe
//! benches (API-50 cells, SW-64 column-wise `DeltaPacked`): a buck stage's inductor current and output and input
//! voltages, sampled once per switching period (400 kHz to 2 MHz). [PowerStage] has three, [PowerStage2] two. No
//! methods: the benches sample and poll from the firmware.
#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
use ww_si::{Ampere, Volt};

#[ww_api_root]
pub trait PowerStage {
    /// Inductor current, the period's average: `Milli` `I32`.
    property!(ro+observe+batched i_l: Ampere);
    /// Output voltage: `Milli` `I32`.
    property!(ro+observe+batched v_out: Volt);
    /// Input voltage: `Milli` `I32`.
    property!(ro+observe+batched v_in: Volt);
}

/// The same stage with two values, for the fastest rate.
#[ww_trait]
pub trait PowerStage2 {
    /// Inductor current, the period's average: `Milli` `I32`.
    property!(ro+observe+batched i_l: Ampere);
    /// Output voltage: `Milli` `I32`.
    property!(ro+observe+batched v_out: Volt);
}

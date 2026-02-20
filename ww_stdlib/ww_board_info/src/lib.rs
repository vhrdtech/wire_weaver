#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
use ww_date_time::DateTime;

/// PCB and PCBA information.
///
/// Leave unavailable fields as empty strings.
#[ww_trait]
pub trait BoardInfo {
    /// PCB series name
    property!(ro name: &'i str);
    /// PCB revision
    property!(ro revision: &'i str);
    /// PCB assembly variant
    property!(ro variant: &'i str);
    /// PCB assembly BOM variant
    property!(ro bom_variant: &'i str);
    /// Serial number
    property!(ro serial: &'i str);
    /// Build date, if available
    property!(ro build_date: Option<DateTime>);
}

#![cfg_attr(not(feature = "std"), no_std)]

use wire_weaver::prelude::*;
pub use ww_date_time::DateTime;

/// PCB and PCBA information.
///
/// Leave unavailable fields as empty strings.
#[ww_trait]
pub trait BoardInfo {
    /// PCB series name
    property!(ro name: str);
    /// PCB revision
    property!(ro revision: str);
    /// PCB assembly variant
    property!(ro variant: str);
    /// PCB assembly BOM variant
    property!(ro bom_variant: str);
    /// Serial number
    property!(ro serial: str);
    /// Build date, if available
    property!(ro build_date: Option<DateTime>);
}

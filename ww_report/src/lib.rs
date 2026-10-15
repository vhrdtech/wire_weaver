//! What a device is and a bug report of it, for any WireWeaver device (CLT-25): the host side of the device contract
//! (`ww_firmware_info`, `ww_board_info`, `ww_uid`, `ww_user_info`, `ww_counters`, `ww_log`).
//!
//! * [info::collect_info] asks the global info traits (API-20) for firmware, board, ids, label and last reset.
//!   Every part says whether the device answered, doesn't implement the trait, or failed: [info::Part].
//! * [report::collect_report] adds the counters, the stored `defmt` log and the crash's addresses, decoded with the
//!   firmware's ELF found by build id (`cnt_core::elf`, filled by `ww elf add`), and [report::render_report] puts
//!   it all in one text.
//!
//! Everything goes through one [clients::Contract], which has to live as long as the device is used: dropping a
//! client disconnects the device.
//!
//! ```ignore
//! let device = DynClient::from_config(config).connect().await?;
//! let contract = Contract::new(&device);
//! let report = collect_report(&contract, &ReportOptions::default()).await;
//! println!("{}", render_report(&report));
//! ```

pub mod clients;
pub mod counters;
pub mod info;
pub mod log;
pub mod path;
pub mod report;
pub mod symbols;

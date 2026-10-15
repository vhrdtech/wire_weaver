//! Clients of the global traits the report reads. Calls carry a global trait path, the device resolves it wherever
//! it implements the trait, whatever its API and version (API-20).

use std::sync::Arc;
use wire_weaver_client::{Commander, DynClient};

macro_rules! trait_client {
    ($client:ident, $module:ident, $krate:ident :: $tr:ident, $gid:path) => {
        /// Client of the trait through a global attachment.
        pub struct $client {
            cmd: Commander,
        }

        impl $client {
            /// Address the trait by an absolute path instead of a global trait path: what streams need.
            pub fn at_path(&mut self, path: Vec<wire_weaver::prelude::UNib32>) {
                self.cmd.set_base_path(path);
            }

            pub fn new(device: &DynClient) -> Self {
                Self {
                    cmd: device
                        .global_attachment($gid, stringify!($tr))
                        .cmd_tx_take(),
                }
            }
        }

        mod $module {
            use super::*;
            wire_weaver::ww_codegen!(
                $krate :: $tr for $client,
                client = "trait_client",
                no_alloc = false,
                use_async = true,
            );
        }
    };
}

trait_client!(
    FirmwareInfoClient,
    firmware_info_client,
    ww_firmware_info::FirmwareInfo,
    ww_firmware_info::FIRMWARE_INFO_FULL_GID
);
trait_client!(
    LastResetClient,
    last_reset_client,
    ww_firmware_info::LastReset,
    ww_firmware_info::LAST_RESET_FULL_GID
);
trait_client!(
    BoardInfoClient,
    board_info_client,
    ww_board_info::BoardInfo,
    ww_board_info::BOARD_INFO_FULL_GID
);
trait_client!(UidClient, uid_client, ww_uid::Uid, ww_uid::UID_FULL_GID);
trait_client!(
    UserInfoClient,
    user_info_client,
    ww_user_info::UserInfo,
    ww_user_info::USER_INFO_FULL_GID
);
trait_client!(
    LogDefmtClient,
    log_defmt_client,
    ww_log::LogDefmt,
    ww_log::LOG_DEFMT_FULL_GID
);
trait_client!(
    CountersClient,
    counters_client,
    ww_counters::Counters,
    ww_counters::COUNTERS_FULL_GID
);

/// Clients of every trait of the device contract, on one connection.
///
/// Keep it for as long as you talk to the device: dropping a client (a [Commander]) disconnects the device, so
/// building one per call would disconnect after the first.
pub struct Contract {
    /// Crate name and version of the device's own API root
    pub api: String,
    pub firmware: FirmwareInfoClient,
    pub last_reset: LastResetClient,
    pub board: BoardInfoClient,
    pub uid: UidClient,
    pub user: UserInfoClient,
    pub defmt: LogDefmtClient,
    /// `defmt` is addressed by the absolute path found in the device's introspection data, which the stored log
    /// stream needs (CLT-23). `false` on a traits-only connection.
    pub defmt_absolute: bool,
    /// Shared with the blocking source counters are read through
    pub counters: Arc<tokio::sync::Mutex<CountersClient>>,
}

impl Contract {
    pub fn new(device: &DynClient) -> Self {
        let mut defmt = LogDefmtClient::new(device);
        let path = device
            .device_introspect()
            .and_then(|b| crate::path::find_trait(&b.api_bundle, "ww_log", "LogDefmt"));
        let defmt_absolute = path.is_some();
        if let Some(path) = path {
            defmt.at_path(path);
        }
        Self {
            api: format!("{:?}", device.device_api_info().user_api_version),
            firmware: FirmwareInfoClient::new(device),
            last_reset: LastResetClient::new(device),
            board: BoardInfoClient::new(device),
            uid: UidClient::new(device),
            user: UserInfoClient::new(device),
            defmt,
            defmt_absolute,
            counters: Arc::new(tokio::sync::Mutex::new(CountersClient::new(device))),
        }
    }
}

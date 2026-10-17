//! `ww info`: what a device is, from the global info traits it implements, wherever in its API and in whatever
//! version of it.

use crate::clients::*;
use serde::Serialize;
use wire_weaver_client::Error;
use wire_weaver_client::ww_client_server::ErrorKindOwned;

/// One part of the info: what the device answered, that it doesn't implement the trait, or why asking failed.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", content = "value", rename_all = "snake_case")]
pub enum Part<T> {
    Value(T),
    /// The trait is not implemented (not in this version of it, or not at all), or an optional item is not served.
    NotImplemented,
    Error(String),
}

impl<T> Part<T> {
    pub fn value(&self) -> Option<&T> {
        match self {
            Part::Value(v) => Some(v),
            _ => None,
        }
    }

    pub(crate) fn from_result<E: Into<Error>>(r: Result<T, E>) -> Self {
        match r {
            Ok(v) => Part::Value(v),
            Err(e) => Part::from_error(e.into()),
        }
    }

    pub(crate) fn from_error(e: Error) -> Self {
        match &e {
            Error::RemoteError(r)
                if matches!(
                    r.kind,
                    ErrorKindOwned::TraitNotFound
                        | ErrorKindOwned::BadPath
                        | ErrorKindOwned::Unimplemented
                ) =>
            {
                Part::NotImplemented
            }
            _ => Part::Error(e.to_string()),
        }
    }
}

/// `ww_firmware_info::FirmwareInfo`, ready to print.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Firmware {
    pub name: String,
    /// `0.5.0-alpha.1`
    pub version: String,
    /// First 7 hex digits of the commit, `-dirty` when it had uncommitted changes
    pub git: Option<String>,
    /// UTC, `2026-10-03 18:20`
    pub built: Option<String>,
    pub profile: String,
    /// Lower-case hex, empty when the firmware didn't keep the note
    pub build_id: String,
    /// Bytes of the compact build record the device holds, when it has one
    pub build_record_bytes: Option<usize>,
}

/// `ww_board_info::BoardInfo`, ready to print.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Board {
    pub name: String,
    pub revision: String,
    /// `record`, `strap 3`, `assumed`
    pub revision_source: String,
    pub variant: String,
    pub bom_variant: String,
    pub serial: String,
    pub build_date: Option<String>,
}

/// One id of `ww_uid::Uid`, the first of the list is the primary one.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UnitId {
    /// `chip`, `mac48`, `eui64`, `uuid` or the device's own name for it
    pub kind: String,
    pub source: String,
    /// Lower-case hex
    pub value: String,
}

/// `ww_user_info::UserInfo`: what the user called the unit.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Label {
    pub name: String,
    pub notes: Part<String>,
    /// Whether the name survives a power cycle
    pub persistent: bool,
}

/// `ww_firmware_info::LastReset`, ready to print. The stack and fault registers are raw: [crate::report] symbolizes
/// them with the ELF.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LastReset {
    /// `PowerOn`, `Panic`, ...
    pub cause: String,
    /// Build id of the firmware that was running, lower-case hex, empty when unknown
    pub build_id: String,
    pub uptime_s: Option<f64>,
    /// Debug text of the fault registers
    pub fault: Option<String>,
    /// Program counter and link register of a Cortex-M fault, or `mepc` and `ra` of a RISC-V trap
    pub fault_addresses: Vec<u32>,
    pub stack: Vec<u32>,
    pub message: String,
    /// Boots since the crash data was saved: 0 when the last reset was the crash itself
    pub boots_since_crash: u32,
    /// Boots since the boot counter was cleared
    pub boot_count: Part<u32>,
}

/// What `ww info` shows: every part is independent, a device implements what it implements.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DeviceInfo {
    /// Crate name and version of the device's own API root, `None` when it isn't known (traits-only connection)
    pub api: Option<String>,
    pub firmware: Part<Firmware>,
    pub board: Part<Board>,
    pub ids: Part<Vec<UnitId>>,
    pub label: Part<Label>,
    pub last_reset: Part<LastReset>,
}

/// Ask the device for everything `ww info` shows. Never fails as a whole: each part says what became of it.
pub async fn collect_info(contract: &Contract) -> DeviceInfo {
    DeviceInfo {
        api: Some(contract.api.clone()),
        firmware: firmware(&contract.firmware).await,
        board: board(&contract.board).await,
        ids: ids(&contract.uid).await,
        label: label(&contract.user).await,
        last_reset: last_reset(&contract.last_reset).await,
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

async fn firmware(client: &FirmwareInfoClient) -> Part<Firmware> {
    let summary = match Part::from_result(client.read_FIRMWARE().read().await) {
        Part::Value(s) => s,
        Part::NotImplemented => return Part::NotImplemented,
        Part::Error(e) => return Part::Error(e),
    };
    let build_id = match client.read_BUILD_ID().read().await {
        Ok(id) => id.as_slice().to_vec(),
        Err(_) => vec![],
    };
    let build_record_bytes = client
        .read_BUILD_RECORD()
        .read()
        .await
        .ok()
        .map(|r| r.as_slice().len());
    let v = &summary.version;
    let mut version = format!("{}.{}.{}", v.major.0, v.minor.0, v.patch.0);
    if let Some(pre) = &v.pre {
        version.push('-');
        version.push_str(pre);
    }
    if let Some(build) = &v.build {
        version.push('+');
        version.push_str(build);
    }
    let git = summary.git.as_ref().map(|g| {
        let hex = hex(g.commit.as_slice());
        let short = &hex[..hex.len().min(7)];
        format!("{short}{}", if g.dirty { "-dirty" } else { "" })
    });
    let profile = match &summary.profile {
        ww_firmware_info::BuildProfileOwned::Release => "release".to_string(),
        ww_firmware_info::BuildProfileOwned::Debug => "debug".to_string(),
        ww_firmware_info::BuildProfileOwned::Other(name) => name.to_string(),
        other => format!("{other:?}"),
    };
    Part::Value(Firmware {
        name: summary.name.to_string(),
        version,
        git,
        built: summary.built.as_ref().map(date_time),
        profile,
        build_id: hex(&build_id),
        build_record_bytes,
    })
}

fn date_time(dt: &ww_firmware_info::DateTime) -> String {
    let secs = dt.time.secs.value();
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        dt.date.year.year(),
        dt.date.month.value(),
        dt.date.day.value(),
        secs / 3600,
        secs / 60 % 60
    )
}

async fn board(client: &BoardInfoClient) -> Part<Board> {
    match Part::from_result(client.read_BOARD().read().await) {
        Part::Value(b) => {
            use ww_board_info::RevisionSource as Source;
            Part::Value(Board {
                name: b.name.to_string(),
                revision: b.revision.to_string(),
                revision_source: match b.revision_source {
                    Source::Record => "record".to_string(),
                    Source::Strap { value } => format!("strap {value}"),
                    Source::Assumed => "assumed".to_string(),
                    other => format!("{other:?}"),
                },
                variant: b.variant.to_string(),
                bom_variant: b.bom_variant.to_string(),
                serial: b.serial.to_string(),
                build_date: b.build_date.as_ref().map(date_time),
            })
        }
        Part::NotImplemented => Part::NotImplemented,
        Part::Error(e) => Part::Error(e),
    }
}

async fn ids(client: &UidClient) -> Part<Vec<UnitId>> {
    use ww_uid::{UidKindOwned as Kind, UidSourceOwned as Source};
    match Part::from_result(client.read_IDS().read().await) {
        Part::Value(ids) => Part::Value(
            ids.iter()
                .map(|id| UnitId {
                    kind: match &id.kind {
                        Kind::ChipUnique => "chip".to_string(),
                        Kind::Mac48 => "mac48".to_string(),
                        Kind::Eui64 => "eui64".to_string(),
                        Kind::Uuid => "uuid".to_string(),
                        Kind::Other(name) => name.to_string(),
                        other => format!("{other:?}"),
                    },
                    source: match &id.source {
                        Source::InternalIc => "internal".to_string(),
                        Source::ExternalIc => "external".to_string(),
                        Source::Firmware => "firmware".to_string(),
                        Source::HardwareStraps => "straps".to_string(),
                        Source::Other(name) => name.to_string(),
                        other => format!("{other:?}"),
                    },
                    value: hex(id.value.as_slice()),
                })
                .collect(),
        ),
        Part::NotImplemented => Part::NotImplemented,
        Part::Error(e) => Part::Error(e),
    }
}

async fn label(client: &UserInfoClient) -> Part<Label> {
    let name = match Part::from_result(client.read_name().read().await) {
        Part::Value(n) => n.to_string(),
        Part::NotImplemented => return Part::NotImplemented,
        Part::Error(e) => return Part::Error(e),
    };
    let persistent = match client.read_CAPABILITIES().read().await {
        Ok(caps) => caps.persistent,
        Err(_) => false,
    };
    let notes = match Part::from_result(client.read_notes().read().await) {
        Part::Value(n) => Part::Value(n.to_string()),
        Part::NotImplemented => Part::NotImplemented,
        Part::Error(e) => Part::Error(e),
    };
    Part::Value(Label {
        name,
        notes,
        persistent,
    })
}

async fn last_reset(client: &LastResetClient) -> Part<LastReset> {
    use ww_firmware_info::Fault;
    let record = match Part::from_result(client.last_reset().call().await) {
        Part::Value(r) => r,
        Part::NotImplemented => return Part::NotImplemented,
        Part::Error(e) => return Part::Error(e),
    };
    let boot_count = Part::from_result(client.boot_count().call().await);
    let (fault, fault_addresses) = match &record.fault {
        Some(Fault::CortexM(c)) => (
            Some(format!(
                "Cortex-M fault: pc {:#010x} lr {:#010x} sp {:#010x} xpsr {:#010x} cfsr {:#010x} hfsr {:#010x} \
                 mmfar {:#010x} bfar {:#010x} exc_return {:#010x}",
                c.pc, c.lr, c.sp, c.xpsr, c.cfsr, c.hfsr, c.mmfar, c.bfar, c.exc_return
            )),
            vec![c.pc, c.lr],
        ),
        Some(Fault::RiscV(r)) => (
            Some(format!(
                "RISC-V trap: mepc {:#010x} mcause {:#010x} mtval {:#010x} ra {:#010x} sp {:#010x}",
                r.mepc, r.mcause, r.mtval, r.ra, r.sp
            )),
            vec![r.mepc, r.ra],
        ),
        Some(other) => (Some(format!("{other:?}")), vec![]),
        None => (None, vec![]),
    };
    Part::Value(LastReset {
        cause: format!("{:?}", record.cause),
        build_id: hex(record.build_id.as_slice()),
        uptime_s: record.uptime.as_ref().map(|s| s.to_f64()),
        fault,
        fault_addresses,
        stack: record.stack.as_slice().to_vec(),
        message: record.message.to_string(),
        boots_since_crash: record.boots_since_crash,
        boot_count,
    })
}

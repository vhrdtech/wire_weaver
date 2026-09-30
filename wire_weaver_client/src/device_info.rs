use std::fmt::Debug;

use semver::Version;
use ww_version::{ApiHashOwned, ApiHashPairOwned};
use ww_version::{FullVersionOwned, VersionOwned};

use crate::config::{ConfigPiece, ConfigPieceDiscriminants};

/// Device information available without opening it (e.g., from USB string descriptors).
#[derive(Debug)]
pub struct DeviceInfo {
    /// Where the device is connected, e.g. `usb 3-1.2 c0de:cafe`.
    pub location: String,
    pub manufacturer: String,
    /// Device description.
    pub product: String,
    pub serials: Vec<String>,
    /// User assigned label, empty if not set.
    pub user_label: String,
    /// API implemented by the device, if it reports one (see [wire_weaver::api_id]).
    pub api: Option<ApiInfo>,
    /// Bus, port chain and VID:PID for USB devices.
    pub usb: Option<UsbLocation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UsbLocation {
    pub bus_id: String,
    pub port_chain: Vec<u8>,
    pub vid: u16,
    pub pid: u16,
}

#[derive(Debug)]
pub struct ApiInfo {
    /// API crate name
    pub gid: String,
    pub version: Version,
    /// Truncated API hash without docs, empty if not reported.
    pub signature: ApiHashOwned,
}

impl ApiInfo {
    fn from_api_id(id: &wire_weaver::api_id::ApiId<'_>) -> Self {
        let v = &id.version;
        let version = Version {
            major: v.major.0 as u64,
            minor: v.minor.0 as u64,
            patch: v.patch.0 as u64,
            pre: v
                .pre
                .and_then(|pre| semver::Prerelease::new(pre).ok())
                .unwrap_or_default(),
            build: v
                .build
                .and_then(|build| semver::BuildMetadata::new(build).ok())
                .unwrap_or_default(),
        };
        ApiInfo {
            gid: id.crate_id.to_string(),
            version,
            signature: ApiHashOwned {
                hash: id.hash.map(|h| h.to_vec()).unwrap_or_default(),
            },
        }
    }
}

impl std::fmt::Display for DeviceInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} \"{}\" \"{}\"",
            self.location, self.manufacturer, self.product
        )?;
        for serial in &self.serials {
            write!(f, " serial={serial}")?;
        }
        if let Some(api) = &self.api {
            write!(f, " api={}@{}", api.gid, api.version)?;
            if !api.signature.hash.is_empty() {
                write!(f, " hash={}", api.signature)?;
            }
        }
        if !self.user_label.is_empty() {
            write!(f, " label=\"{}\"", self.user_label)?;
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct ConnectionInfo {
    pub result: Result<DeviceApiInfo, anyhow::Error>,
}

#[derive(Clone, Debug)]
pub struct DeviceApiInfo {
    /// Link carries API model messages (e.g., ww_link).
    pub link_version: FullVersionOwned,
    /// Maximum message size supported by the device.
    pub max_message_size: usize,
    /// API model defines what operations can be performed (call, write, etc. from e.g., ww_client_server).
    pub api_model_version: FullVersionOwned,
    /// User-defined API carried by API model.
    pub user_api_version: FullVersionOwned,
    pub user_api_hash: ApiHashPairOwned,
}

impl DeviceApiInfo {
    pub fn empty() -> Self {
        DeviceApiInfo {
            link_version: FullVersionOwned::new("".into(), VersionOwned::new(0, 0, 0)),
            max_message_size: 0,
            api_model_version: FullVersionOwned::new("".into(), VersionOwned::new(0, 0, 0)),
            user_api_version: FullVersionOwned::new("".into(), VersionOwned::new(0, 0, 0)),
            user_api_hash: ApiHashPairOwned {
                no_docs: ApiHashOwned { hash: vec![] },
                with_docs: ApiHashOwned { hash: vec![] },
            },
        }
    }
}

impl ConnectionInfo {
    pub fn err(e: anyhow::Error) -> Self {
        Self { result: Err(e) }
    }
}

impl DeviceInfo {
    /// Whether the device passes the filters in `pieces`: filters of the same kind are alternatives (any of them
    /// has to match), filters of different kinds are all required. No filters at all match any device.
    pub(crate) fn is_matching(&self, pieces: &[ConfigPiece]) -> bool {
        use ConfigPieceDiscriminants as D;
        const FILTERS: [D; 8] = [
            D::UsbVidPid,
            D::UsbPath,
            D::SerialEq,
            D::SerialContains,
            D::UserLabelEq,
            D::ManufacturerContains,
            D::ProductContains,
            D::ImplementsApi,
        ];
        FILTERS.iter().all(|kind| {
            let mut of_kind = pieces.iter().filter(|p| D::from(*p) == *kind).peekable();
            of_kind.peek().is_none() || of_kind.any(|p| self.matches_filter(p))
        })
    }

    fn matches_filter(&self, piece: &ConfigPiece) -> bool {
        match piece {
            ConfigPiece::UsbVidPid { vid, pid } => self
                .usb
                .as_ref()
                .is_some_and(|u| u.vid == *vid && u.pid == *pid),
            ConfigPiece::UsbPath { bus_id, port_chain } => self
                .usb
                .as_ref()
                .is_some_and(|u| &u.bus_id == bus_id && &u.port_chain == port_chain),
            ConfigPiece::SerialEq { serial } => self.serials.iter().any(|s| eq(s, serial)),
            ConfigPiece::SerialContains { substring } => {
                self.serials.iter().any(|s| contains(s, substring))
            }
            ConfigPiece::UserLabelEq { user_label } => eq(&self.user_label, user_label),
            ConfigPiece::ManufacturerContains { substring } => {
                contains(&self.manufacturer, substring)
            }
            ConfigPiece::ProductContains { substring } => contains(&self.product, substring),
            ConfigPiece::ImplementsApi {
                api_gid,
                version_req,
            } => self
                .api
                .as_ref()
                .is_some_and(|api| &api.gid == api_gid && version_req.matches(&api.version)),
            _ => true,
        }
    }
}

fn contains(s: &str, substring: &str) -> bool {
    s.to_lowercase().contains(&substring.to_lowercase())
}

fn eq(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

#[cfg(feature = "usb")]
impl From<&nusb::DeviceInfo> for DeviceInfo {
    fn from(info: &nusb::DeviceInfo) -> Self {
        let manufacturer = info.manufacturer_string().unwrap_or_default().to_string();
        let product = info.product_string().unwrap_or_default().to_string();
        let raw_serial = info.serial_number().unwrap_or_default().to_string();
        // WireWeaver interface string carries API id, fall back to product string for devices that put it there
        let api_id = info
            .interfaces()
            .filter_map(|i| i.interface_string())
            .chain(info.product_string())
            .find_map(wire_weaver::api_id::parse);
        let port_chain = info
            .port_chain()
            .iter()
            .map(|p| p.to_string())
            .collect::<Vec<_>>()
            .join(".");
        DeviceInfo {
            location: format!(
                "usb {}-{} {:04x}:{:04x}",
                info.bus_id(),
                port_chain,
                info.vendor_id(),
                info.product_id()
            ),
            manufacturer,
            product,
            serials: vec![raw_serial],
            user_label: api_id
                .and_then(|id| id.label)
                .unwrap_or_default()
                .to_string(),
            api: api_id.as_ref().map(ApiInfo::from_api_id),
            usb: Some(UsbLocation {
                bus_id: info.bus_id().to_string(),
                port_chain: info.port_chain().to_vec(),
                vid: info.vendor_id(),
                pid: info.product_id(),
            }),
        }
    }
}

#[cfg(feature = "rtt")]
impl From<&probe_rs::probe::DebugProbeInfo> for DeviceInfo {
    fn from(info: &probe_rs::probe::DebugProbeInfo) -> Self {
        DeviceInfo {
            location: format!("probe {:04x}:{:04x}", info.vendor_id, info.product_id),
            manufacturer: "".into(),
            product: info.identifier.clone(),
            serials: info
                .serial_number
                .clone()
                .map(|s| vec![s])
                .unwrap_or_default(),
            user_label: "".into(),
            api: None,
            usb: None,
        }
    }
}

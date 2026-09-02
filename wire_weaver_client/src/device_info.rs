use std::fmt::Debug;

use semver::Version;
use ww_version::{ApiHashOwned, ApiHashPairOwned};
use ww_version::{FullVersionOwned, VersionOwned};

use crate::config::ValidatedConfig;

#[derive(Debug)]
pub struct DeviceInfo {
    pub manufacturer: String,
    pub product: String,
    pub serials: Vec<String>,
    pub user_label: String,
    pub api: Option<ApiInfo>,
}

#[derive(Debug)]
pub struct ApiInfo {
    pub gid: String,
    pub version: Version,
    pub signature: ApiHashOwned,
}

#[derive(Debug)]
pub struct ConnectionInfo {
    pub result: Result<DeviceApiInfo, anyhow::Error>,
}

#[derive(Clone, Debug)]
pub struct DeviceApiInfo {
    /// Link carries API model messages (e.g., wire_weaver_usb_link).
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
    pub(crate) fn is_matching(&self, f: &ValidatedConfig) -> bool {
        let mfg_match = Self::contains(&self.manufacturer, f.manufacturers_contains());
        let product_match = Self::contains(&self.product, f.products_contains());
        let serials_match = self.serials.iter().any(|s| Self::eq(s, f.serials_eq()));
        let labels_match = Self::eq(&self.user_label, f.user_labels_eq());
        let api_match = if let Some(api) = &self.api {
            f.implements_api()
                .into_iter()
                .any(|(api_gid, req)| api_gid == api.gid && req.matches(&api.version))
        } else {
            false
        };
        mfg_match || product_match || serials_match || labels_match || api_match
    }

    fn contains<'i>(s: &str, substrings: impl Iterator<Item = &'i str>) -> bool {
        let s = s.to_lowercase();
        substrings
            .into_iter()
            .any(|substr| s.contains(&substr.to_lowercase()))
    }

    fn eq<'i>(s: &str, substrings: impl Iterator<Item = &'i str>) -> bool {
        let s = s.to_lowercase();
        substrings.into_iter().any(|s2| s == s2.to_lowercase())
    }
}

#[cfg(feature = "usb")]
impl From<&nusb::DeviceInfo> for DeviceInfo {
    fn from(info: &nusb::DeviceInfo) -> Self {
        let manufacturer = info.manufacturer_string().unwrap_or_default().to_string();
        let product = info.product_string().unwrap_or_default().to_string();
        let raw_serial = info.serial_number().unwrap_or_default().to_string();
        // TODO: parse serials, versions, labels, etc.
        DeviceInfo {
            manufacturer,
            product,
            serials: vec![raw_serial],
            user_label: "".into(),
            api: None,
        }
    }
}

#[cfg(feature = "rtt")]
impl From<&probe_rs::probe::DebugProbeInfo> for DeviceInfo {
    fn from(info: &probe_rs::probe::DebugProbeInfo) -> Self {
        DeviceInfo {
            manufacturer: "".into(),
            product: info.identifier.clone(),
            serials: info
                .serial_number
                .clone()
                .map(|s| vec![s])
                .unwrap_or_default(),
            user_label: "".into(),
            api: None,
        }
    }
}

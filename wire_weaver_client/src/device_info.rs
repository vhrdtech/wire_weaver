use semver::Version;

use crate::config::ValidatedConfig;

pub struct DeviceInfo {
    pub manufacturer: String,
    pub product: String,
    pub serials: Vec<String>,
    pub user_label: String,
    pub api: Option<ApiInfo>,
}

pub struct ApiInfo {
    pub gid: String,
    pub version: Version,
    pub signature: Vec<u8>,
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

use crate::ast::api::ApiLevelSourceLocation;
use ron::ser::{to_string_pretty, PrettyConfig};
use ww_self::ApiBundleOwned;

pub(crate) fn cache_api_bundle(source: &ApiLevelSourceLocation, api_bundle: &ApiBundleOwned) {
    let Ok(_as_ron) = to_string_pretty(&api_bundle, PrettyConfig::new().compact_structs(true))
    else {
        return;
    };
    // let api_crate_name = source.crate_name();
}

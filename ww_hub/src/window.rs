//! Which clients this hub's build serves (SRV-30): the hub reads only the envelope of its clients' messages (link
//! messages, `ww_client_server` requests and events) and passes the user API through, so what has to match is a
//! client's `ww_link` and `ww_client_server` versions, which it says in its preamble. A hub serves every version its
//! build is compatible with by Cargo's SemVer rule, so a newer hub keeps serving older clients of the same major
//! (0.x: minor). When a release breaks `ww_link` or `ww_client_server`, the hub keeps the previous major's code for
//! one major and lists it in [LINK_WINDOW] / [API_MODEL_WINDOW], the way a device keeps an old protocol.

use ww_local_socket::{Preamble, Version};

/// Earlier `ww_link` versions served besides those compatible with this build's: none yet.
const LINK_WINDOW: &[Version] = &[];
/// Earlier `ww_client_server` versions served besides those compatible with this build's: none yet.
const API_MODEL_WINDOW: &[Version] = &[];

fn version(v: wire_weaver::ww_version::CompactVersion) -> Version {
    Version::new(v.major.0, v.minor.0, v.patch.0)
}

/// This build's `ww_link` version.
pub(crate) fn link() -> Version {
    version(ww_device::ww_link::LINK_VERSION)
}

/// This build's `ww_client_server` version.
pub(crate) fn api_model() -> Version {
    version(ww_client_server::COMPACT_VERSION)
}

fn served(own: Version, window: &[Version], client: Option<Version>) -> bool {
    // a client from before the versions were in the preamble: served as before, link setup checks it
    let Some(client) = client else {
        return true;
    };
    own.is_compatible(&client) || window.iter().any(|w| w.is_compatible(&client))
}

/// Whether the client of `preamble` is served; `Err` says which version isn't.
pub(crate) fn check(preamble: &Preamble) -> Result<(), String> {
    check_with(preamble, link(), api_model())
}

fn check_with(preamble: &Preamble, link: Version, api_model: Version) -> Result<(), String> {
    if !served(link, LINK_WINDOW, preamble.link_version) {
        return Err(format!(
            "ww_link {} (this hub serves {link})",
            preamble.link_version.expect("served without a version")
        ));
    }
    if !served(api_model, API_MODEL_WINDOW, preamble.api_model_version) {
        return Err(format!(
            "ww_client_server {} (this hub serves {api_model})",
            preamble
                .api_model_version
                .expect("served without a version")
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ww_local_socket::Target;

    #[test]
    fn older_compatible_clients_are_served_others_are_not() {
        let (link, model) = (Version::new(0, 5, 3), Version::new(0, 2, 1));
        let p = |l, m| Preamble::new(Target::Hub).with_versions(l, m);
        assert!(
            check_with(
                &p(Version::new(0, 5, 0), Version::new(0, 2, 0)),
                link,
                model
            )
            .is_ok()
        );
        let e = check_with(&p(Version::new(0, 4, 9), model), link, model).unwrap_err();
        assert_eq!(e, "ww_link 0.4.9 (this hub serves 0.5.3)");
        let e = check_with(&p(link, Version::new(0, 3, 0)), link, model).unwrap_err();
        assert!(e.starts_with("ww_client_server 0.3.0"), "{e}");
        // version 1: unknown versions, served
        assert!(check_with(&Preamble::new(Target::Hub).v1(), link, model).is_ok());
        assert!(check(&p(self::link(), self::api_model())).is_ok());
    }
}

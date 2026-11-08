//! Which clients this hub's build serves (SRV-30, EVO-23): the hub reads only the envelope of its clients' messages
//! (link messages, `ww_client_server` requests and events, all shrink_wrap) and passes the user API through, so what
//! has to match is a client's link, shrink_wrap and API model layers. A client says their generations in its
//! preamble, the same bytes as in its link hello; a generation counts only breaks, so a client of the hub's
//! generations is served whatever its versions. When a release breaks one of these layers (its generation goes up),
//! the hub keeps the previous generation's code for a while and lists it in [LINK_WINDOW] / [SHRINK_WRAP_WINDOW] /
//! [API_MODEL_WINDOW], the way a device keeps an old protocol. The framer's generation doesn't matter here: the local
//! socket carries `len | kind | payload`, not framer frames.
//!
//! A client from before the generations (EVO-23) is judged by its `ww_link` and `ww_client_server` versions by Cargo's
//! SemVer rule, as before; one from before the versions (SRV-30) is served, and link setup checks it.

use ww_device::ww_link::Generations;
use ww_local_socket::{Preamble, Version};

/// Earlier link generations served besides this build's: none yet.
const LINK_WINDOW: &[u8] = &[];
/// Earlier shrink_wrap generations served besides this build's: none yet.
const SHRINK_WRAP_WINDOW: &[u8] = &[];
/// Earlier API model (`ww_client_server`) generations served besides this build's: none yet.
const API_MODEL_WINDOW: &[u8] = &[];

/// This build's generations.
pub(crate) fn generations() -> Generations {
    Generations::this_build(ww_client_server::GENERATION)
}

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

fn version_served(own: Version, client: Option<Version>) -> bool {
    client.is_none_or(|client| own.is_compatible(&client))
}

/// Whether the client of `preamble` is served; `Err` says which layer isn't.
pub(crate) fn check(preamble: &Preamble) -> Result<(), String> {
    check_with(preamble, generations(), link(), api_model())
}

fn check_with(
    preamble: &Preamble,
    own: Generations,
    link: Version,
    api_model: Version,
) -> Result<(), String> {
    if let Some(client) = preamble.generations {
        for (name, ours, window, theirs) in [
            ("link", own.link, LINK_WINDOW, client.link),
            (
                "shrink_wrap",
                own.shrink_wrap,
                SHRINK_WRAP_WINDOW,
                client.shrink_wrap,
            ),
            (
                "API model",
                own.api_model,
                API_MODEL_WINDOW,
                client.api_model,
            ),
        ] {
            if theirs != ours && !window.contains(&theirs) {
                return Err(format!(
                    "{name} generation {theirs} (this hub serves generation {ours})"
                ));
            }
        }
        return Ok(());
    }
    if !version_served(link, preamble.link_version) {
        return Err(format!(
            "ww_link {} (this hub serves {link})",
            preamble.link_version.expect("served without a version")
        ));
    }
    if !version_served(api_model, preamble.api_model_version) {
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

    const LINK: Version = Version::new(0, 5, 3);
    const MODEL: Version = Version::new(0, 2, 1);

    fn gens(link: u8, shrink_wrap: u8, api_model: u8) -> ww_local_socket::Generations {
        ww_local_socket::Generations {
            framer: 1,
            link,
            shrink_wrap,
            api_model,
        }
    }

    fn own() -> Generations {
        Generations {
            framer: 1,
            link: 1,
            shrink_wrap: 1,
            api_model: 1,
        }
    }

    /// With generations, they decide: other versions of the same generations are served, another generation of
    /// any layer the hub reads is not, the framer's doesn't matter.
    #[test]
    fn clients_are_served_by_generation() {
        let p = |g| {
            Preamble::new(Target::Hub)
                .with_versions(Version::new(0, 9, 0), Version::new(0, 7, 0))
                .with_generations(g)
        };
        assert!(check_with(&p(gens(1, 1, 1)), own(), LINK, MODEL).is_ok());
        let framer_2 = ww_local_socket::Generations {
            framer: 2,
            ..gens(1, 1, 1)
        };
        assert!(check_with(&p(framer_2), own(), LINK, MODEL).is_ok());
        let e = check_with(&p(gens(2, 1, 1)), own(), LINK, MODEL).unwrap_err();
        assert_eq!(e, "link generation 2 (this hub serves generation 1)");
        let e = check_with(&p(gens(1, 2, 1)), own(), LINK, MODEL).unwrap_err();
        assert_eq!(e, "shrink_wrap generation 2 (this hub serves generation 1)");
        let e = check_with(&p(gens(1, 1, 0)), own(), LINK, MODEL).unwrap_err();
        assert_eq!(e, "API model generation 0 (this hub serves generation 1)");
    }

    /// A client from before the generations is judged by its versions.
    #[test]
    fn older_clients_by_version() {
        let p = |l, m| Preamble::new(Target::Hub).with_versions(l, m);
        assert!(
            check_with(
                &p(Version::new(0, 5, 0), Version::new(0, 2, 0)),
                own(),
                LINK,
                MODEL
            )
            .is_ok()
        );
        let e = check_with(&p(Version::new(0, 4, 9), MODEL), own(), LINK, MODEL).unwrap_err();
        assert_eq!(e, "ww_link 0.4.9 (this hub serves 0.5.3)");
        let e = check_with(&p(LINK, Version::new(0, 3, 0)), own(), LINK, MODEL).unwrap_err();
        assert!(e.starts_with("ww_client_server 0.3.0"), "{e}");
        // version 1: unknown versions, served
        assert!(check_with(&Preamble::new(Target::Hub).v1(), own(), LINK, MODEL).is_ok());
    }

    /// This build serves itself, as a client of it says.
    #[test]
    fn serves_its_own_build() {
        let g = generations();
        let p = Preamble::new(Target::Hub)
            .with_versions(link(), api_model())
            .with_generations(ww_local_socket::Generations {
                framer: g.framer,
                link: g.link,
                shrink_wrap: g.shrink_wrap,
                api_model: g.api_model,
            });
        assert!(check(&p).is_ok());
    }
}

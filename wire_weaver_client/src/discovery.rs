//! Servers found on the LAN over DNS-SD as `_wireweaver._tcp` (CLT-82, feature `dns_sd`): `ww_dns_sd`'s
//! [Discovery] keeps their list current as they come, change address and go, and [urls] gives a
//! [ClientConfig](crate::ClientConfig) the URLs of one of them at each connect attempt.
//!
//! On Android an app must hold a `WifiManager.MulticastLock` while discovery runs, or the Wi-Fi driver drops the
//! multicast answers.
//!
//! What the LAN announces is unauthenticated: any device can announce any name. Link setup still checks the API.

pub use ww_dns_sd::{Browsed, Discovery, Feed, FoundServer, MdnsOptions, SERVERS_MAX, Select};

use crate::config::UrlSource;

/// The WebSocket URLs of the server `select` picks in `discovery`, looked up at each connect attempt, then
/// `fallback` (saved addresses, a VPN one): for [ClientConfig::websocket_urls_from](crate::ClientConfig::websocket_urls_from).
pub fn urls(discovery: &Discovery, select: Select, fallback: Vec<String>) -> UrlSource {
    let discovery = discovery.clone();
    UrlSource::new(format!("the LAN: {select}"), move || {
        let mut urls = discovery
            .find(&select)
            .map(|s| s.websocket_urls())
            .unwrap_or_default();
        for url in &fallback {
            if !urls.contains(url) {
                urls.push(url.clone());
            }
        }
        urls
    })
}

#[cfg(test)]
mod tests {
    use std::net::IpAddr;
    use std::num::NonZeroU16;

    use super::*;

    fn tv(ip: &str) -> FoundServer {
        let ip: IpAddr = ip.parse().unwrap();
        FoundServer::new(
            "Living room TV",
            NonZeroU16::new(7590).unwrap(),
            "/ww",
            [ip],
        )
    }

    #[test]
    fn urls_follow_the_server_then_the_fallback() {
        let (discovery, feed) = Discovery::fed();
        let fallback = vec!["ws://100.64.0.5:7590/ww".to_owned()];
        let source = urls(
            &discovery,
            Select::Name("Living room TV".to_owned()),
            fallback.clone(),
        );
        assert_eq!(source.urls(), fallback, "not found yet: the fallback alone");
        feed.found(tv("192.168.1.5"));
        assert_eq!(
            source.urls(),
            ["ws://192.168.1.5:7590/ww", "ws://100.64.0.5:7590/ww"]
        );
        feed.found(tv("192.168.1.77"));
        assert_eq!(source.urls()[0], "ws://192.168.1.77:7590/ww");
        feed.gone("Living room TV");
        assert_eq!(source.urls(), fallback);
    }
}

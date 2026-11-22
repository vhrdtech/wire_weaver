//! [Browser]: finds `_wireweaver._tcp` servers on the LAN with the in-process `mdns-sd` daemon.
//!
//! Everything a browser reads comes from any device on the LAN, unauthenticated: a [FoundServer] is checked against
//! the same limits a [crate::ServiceRecord] keeps, and a record that breaks them is skipped.

use std::cmp::Ordering;
use std::net::IpAddr;
use std::num::NonZeroU16;
use std::time::{Duration, Instant};

use mdns_sd::{IfKind, ResolvedService, ServiceDaemon, ServiceEvent};

use crate::mdns::{MDNS_PORT, ty_domain};
use crate::{ADDRESSES_MAX, Error, INSTANCE_NAME_LEN_MAX, InstanceId, PATH_LEN_MAX};

/// Where the in-process `mdns-sd` daemon listens.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MdnsOptions {
    /// UDP port: [MDNS_PORT] on a real LAN, a free port of its own in tests, so the LAN sees nothing.
    pub port: u16,
    /// Only the IPv4 loopback interface, with multicast loop on: a browser and a responder in one test.
    pub loopback_only: bool,
}

impl Default for MdnsOptions {
    fn default() -> Self {
        Self {
            port: MDNS_PORT,
            loopback_only: false,
        }
    }
}

impl MdnsOptions {
    /// Starts a daemon with these options.
    pub(crate) fn start(&self) -> Result<ServiceDaemon, Error> {
        let daemon = ServiceDaemon::new_with_port(self.port)?;
        if self.loopback_only {
            daemon.disable_interface(IfKind::All)?;
            daemon.enable_interface(IfKind::LoopbackV4)?;
            daemon.set_multicast_loop_v4(true)?;
        }
        Ok(daemon)
    }
}

/// One server as a browser found it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FoundServer {
    instance: String,
    port: NonZeroU16,
    path: String,
    api: Option<String>,
    wire: Option<String>,
    id: Option<InstanceId>,
    addresses: Vec<IpAddr>,
}

impl FoundServer {
    /// A server found some other way (a test's fake browser): `addresses` are cut to [ADDRESSES_MAX].
    pub fn new(
        instance: impl Into<String>,
        port: NonZeroU16,
        path: impl Into<String>,
        addresses: impl IntoIterator<Item = IpAddr>,
    ) -> Self {
        let mut addresses: Vec<IpAddr> = addresses.into_iter().take(ADDRESSES_MAX).collect();
        sort_addresses(&mut addresses);
        Self {
            instance: instance.into(),
            port,
            path: path.into(),
            api: None,
            wire: None,
            id: None,
            addresses,
        }
    }

    /// The same server with the TXT keys `api` (`blinky_api@0.1.0`), `wire` (`w1.0`) and `id`.
    #[must_use]
    pub fn with_txt(
        mut self,
        api: Option<String>,
        wire: Option<String>,
        id: Option<InstanceId>,
    ) -> Self {
        self.api = api;
        self.wire = wire;
        self.id = id;
        self
    }

    /// Reads a resolved service: `None` when it breaks a limit (name, path, no usable address).
    fn from_resolved(r: &ResolvedService) -> Option<Self> {
        let instance = instance_of(r.get_fullname())?;
        let port = NonZeroU16::new(r.get_port())?;
        let path = r.get_property_val_str("path").unwrap_or("/");
        if !path.starts_with('/')
            || path.len() > PATH_LEN_MAX
            || !path.bytes().all(|b| b.is_ascii_graphic())
        {
            return None;
        }
        let text = |key: &str| {
            r.get_property_val_str(key)
                .filter(|v| !v.is_empty() && v.len() <= crate::TXT_ENTRY_LEN_MAX)
                .map(str::to_owned)
        };
        let id = r
            .get_property_val_str("id")
            .and_then(|id| InstanceId::new(id).ok());
        // link-local IPv6 needs its interface in a URL; the announcer leaves it out too
        let addresses = r
            .get_addresses()
            .iter()
            .map(mdns_sd::ScopedIp::to_ip_addr)
            .filter(|ip| {
                !matches!(ip, IpAddr::V6(v6) if v6.is_unicast_link_local()) && !ip.is_unspecified()
            });
        let found =
            Self::new(instance, port, path, addresses).with_txt(text("api"), text("wire"), id);
        (!found.addresses.is_empty()).then_some(found)
    }

    /// Instance name, like `Living room TV`.
    pub fn instance(&self) -> &str {
        &self.instance
    }

    /// TCP port of the WebSocket server.
    pub fn port(&self) -> NonZeroU16 {
        self.port
    }

    /// Request path of the WebSocket upgrade, `/ww`.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The API with its version, `blinky_api@0.1.0`, when announced.
    pub fn api(&self) -> Option<&str> {
        self.api.as_deref()
    }

    /// The API crate alone, `blinky_api`.
    pub fn api_crate(&self) -> Option<&str> {
        self.api
            .as_deref()
            .map(|api| api.split_once('@').map_or(api, |(name, _)| name))
    }

    /// Wire version of the API root, `w1.0`, when announced.
    pub fn wire(&self) -> Option<&str> {
        self.wire.as_deref()
    }

    /// The instance id, when announced and well-formed.
    pub fn id(&self) -> Option<&InstanceId> {
        self.id.as_ref()
    }

    /// Addresses, IPv4 first, each kind in ascending order.
    pub fn addresses(&self) -> &[IpAddr] {
        &self.addresses
    }

    /// `HOST:PORT` of each address, IPv6 in brackets.
    pub fn socket_addresses(&self) -> Vec<String> {
        self.addresses
            .iter()
            .map(|ip| match ip {
                IpAddr::V4(v4) => format!("{v4}:{}", self.port),
                IpAddr::V6(v6) => format!("[{v6}]:{}", self.port),
            })
            .collect()
    }

    /// `ws://HOST:PORT/PATH` of each address, in [Self::addresses]'s order.
    pub fn websocket_urls(&self) -> Vec<String> {
        self.socket_addresses()
            .into_iter()
            .map(|host_port| format!("ws://{host_port}{}", self.path))
            .collect()
    }
}

fn sort_addresses(addresses: &mut Vec<IpAddr>) {
    addresses.sort_unstable_by(|a, b| match (a, b) {
        (IpAddr::V4(_), IpAddr::V6(_)) => Ordering::Less,
        (IpAddr::V6(_), IpAddr::V4(_)) => Ordering::Greater,
        _ => a.cmp(b),
    });
    addresses.dedup();
}

/// The readable instance name of a full DNS-SD name, `None` when it is not one of ours or too long.
fn instance_of(fullname: &str) -> Option<String> {
    let escaped = fullname.strip_suffix(&format!(".{}", ty_domain()))?;
    let name = mdns_sd::instance_name_unescaped(escaped);
    (!name.is_empty() && name.len() <= INSTANCE_NAME_LEN_MAX && !name.chars().any(char::is_control))
        .then_some(name)
}

/// What a browser saw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Browsed {
    /// A server appeared, or changed address, port or TXT keys.
    Found(FoundServer),
    /// The server of this instance name went: withdrawn, or its records expired.
    Gone(String),
}

/// Finds `_wireweaver._tcp` servers on the LAN: the in-process `mdns-sd` daemon on its own thread, browsing from
/// creation on and stopping when dropped. [Self::next] hands out what it saw.
pub struct Browser {
    daemon: ServiceDaemon,
    events: mdns_sd::Receiver<ServiceEvent>,
}

impl Browser {
    /// Starts browsing on the standard port, every interface.
    pub fn new() -> Result<Self, Error> {
        Self::with_options(&MdnsOptions::default())
    }

    /// Starts browsing with `options`.
    pub fn with_options(options: &MdnsOptions) -> Result<Self, Error> {
        let daemon = options.start()?;
        let events = daemon.browse(&ty_domain())?;
        Ok(Self { daemon, events })
    }

    /// The next server found or gone, waiting at most `timeout`: `None` when nothing came.
    pub fn next(&self, timeout: Duration) -> Result<Option<Browsed>, Error> {
        let deadline = Instant::now() + timeout;
        loop {
            let event = match self.events.recv_deadline(deadline) {
                Ok(event) => event,
                Err(mdns_sd::RecvTimeoutError::Timeout) => return Ok(None),
                Err(mdns_sd::RecvTimeoutError::Disconnected) => return Err(Error::BrowserGone),
            };
            match event {
                ServiceEvent::ServiceResolved(r) => match FoundServer::from_resolved(&r) {
                    Some(found) => return Ok(Some(Browsed::Found(found))),
                    None => {
                        tracing::debug!(name = r.get_fullname(), "skipped: breaks a DNS-SD limit")
                    }
                },
                ServiceEvent::ServiceRemoved(_, fullname) => {
                    if let Some(name) = instance_of(&fullname) {
                        return Ok(Some(Browsed::Gone(name)));
                    }
                }
                // search notices, and services found but not resolved yet
                _ => {}
            }
        }
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        if let Err(e) = self.daemon.stop_browse(&ty_domain()) {
            tracing::debug!("stopping the DNS-SD browse failed: {e}");
        }
        match self.daemon.shutdown() {
            Ok(done) => {
                if done.recv_timeout(crate::mdns::CONFIRM_TIMEOUT).is_err() {
                    tracing::warn!("the mDNS browser's thread did not confirm its shutdown");
                }
            }
            Err(e) => tracing::warn!("stopping the mDNS browser failed: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_names_are_unescaped_and_checked() {
        assert_eq!(
            instance_of("Living room TV._wireweaver._tcp.local.").as_deref(),
            Some("Living room TV")
        );
        assert_eq!(
            instance_of(r"tv\.2._wireweaver._tcp.local.").as_deref(),
            Some("tv.2")
        );
        assert_eq!(instance_of("tv._http._tcp.local."), None);
        let long = format!(
            "{}._wireweaver._tcp.local.",
            "x".repeat(INSTANCE_NAME_LEN_MAX + 1)
        );
        assert_eq!(instance_of(&long), None);
    }

    #[test]
    fn urls_put_ipv6_in_brackets_after_ipv4() {
        let v6: IpAddr = "fd00::5".parse().unwrap();
        let v4: IpAddr = "192.168.1.5".parse().unwrap();
        let found = FoundServer::new("tv", NonZeroU16::new(7590).unwrap(), "/ww", [v6, v4, v6])
            .with_txt(Some("ww_niri_api@0.3.1".to_owned()), None, None);
        assert_eq!(found.addresses(), [v4, v6]);
        assert_eq!(
            found.websocket_urls(),
            ["ws://192.168.1.5:7590/ww", "ws://[fd00::5]:7590/ww"]
        );
        assert_eq!(found.api_crate(), Some("ww_niri_api"));
    }
}

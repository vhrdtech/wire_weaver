//! Announces WireWeaver WebSocket servers on the LAN over DNS-SD (mDNS), as service type `_wireweaver._tcp`, so
//! clients find them without a typed address.
//!
//! A [ServiceRecord] holds the instance name, the port, the TXT keys ([Txt]: `path`, `api`, `wire`, `id`) and the
//! addresses to announce. An [Advertiser] keeps it announced through a [Responder]: published while it has
//! addresses, updated by [Advertiser::set_addresses], withdrawn when dropped. Responders, each behind a feature:
//!
//! - [MdnsResponder] (feature `mdns`, on by default): in-process, the `mdns-sd` crate on its own thread, nothing
//!   to install.
//! - `AvahiResponder` (feature `avahi`): the system's Avahi daemon over D-Bus, where an in-process responder can't
//!   run.
//!
//! [start_responder] picks one. Nothing here needs an async runtime: calls block briefly, on any thread or task.
//!
//! Everything announced reaches every device on the LAN, unauthenticated: the server's address and port, the
//! API it serves and the instance id. See `docs/transport/websocket.md` in the repository.

mod advertiser;
#[cfg(feature = "avahi")]
mod avahi;
#[cfg(feature = "mdns")]
mod mdns;
mod record;

use std::net::IpAddr;

pub use advertiser::{Advertiser, Responder};
#[cfg(feature = "avahi")]
pub use avahi::AvahiResponder;
#[cfg(feature = "mdns")]
pub use mdns::{MDNS_PORT, MdnsResponder};
pub use record::{
    ADDRESSES_MAX, INSTANCE_ID_LEN_MAX, INSTANCE_NAME_LEN_MAX, InstanceId, InstanceName,
    PATH_LEN_MAX, RecordError, SERVICE_TYPE, ServiceRecord, TXT_ENTRY_LEN_MAX, Txt,
};

/// The addresses to announce for `interface` (`eth0`), or for every interface but loopback when `None`. IPv6
/// link-local addresses are left out: a client can't use one without knowing its interface.
pub fn interface_addresses(interface: Option<&str>) -> std::io::Result<Vec<IpAddr>> {
    let mut list = Vec::new();
    for i in if_addrs::get_if_addrs()? {
        let wanted = match interface {
            Some(name) => i.name == name,
            None => !i.is_loopback(),
        };
        let ip = i.ip();
        let link_local_v6 = matches!(ip, IpAddr::V6(v6) if v6.is_unicast_link_local());
        if wanted && !link_local_v6 && !list.contains(&ip) {
            list.push(ip);
        }
    }
    Ok(list)
}

/// Which responder [start_responder] starts.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum ResponderChoice {
    /// The in-process one, Avahi when it fails to start.
    #[default]
    Auto,
    /// Only the in-process one.
    InProcess,
    /// Only Avahi.
    Avahi,
}

/// Starts the responder `choice` names, from those compiled in.
pub fn start_responder(choice: ResponderChoice) -> Result<Box<dyn Responder>, Error> {
    match choice {
        ResponderChoice::InProcess => in_process(),
        ResponderChoice::Avahi => avahi(),
        ResponderChoice::Auto => in_process().or_else(|in_process| {
            avahi().map_err(|avahi| Error::NoResponder {
                in_process: Box::new(in_process),
                avahi: Box::new(avahi),
            })
        }),
    }
}

#[cfg(feature = "mdns")]
fn in_process() -> Result<Box<dyn Responder>, Error> {
    Ok(Box::new(MdnsResponder::new()?))
}

#[cfg(not(feature = "mdns"))]
fn in_process() -> Result<Box<dyn Responder>, Error> {
    Err(Error::NotCompiledIn("mdns"))
}

#[cfg(feature = "avahi")]
fn avahi() -> Result<Box<dyn Responder>, Error> {
    Ok(Box::new(AvahiResponder::new()?))
}

#[cfg(not(feature = "avahi"))]
fn avahi() -> Result<Box<dyn Responder>, Error> {
    Err(Error::NotCompiledIn("avahi"))
}

/// Why a record could not be announced or withdrawn.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The record breaks a DNS limit.
    #[error(transparent)]
    Record(#[from] RecordError),
    /// The in-process responder refused or failed.
    #[cfg(feature = "mdns")]
    #[error("mDNS responder: {0}")]
    Mdns(#[from] mdns_sd::Error),
    /// The in-process responder's thread did not answer in time.
    #[error("the mDNS responder's thread did not answer")]
    ResponderGone,
    /// D-Bus or Avahi refused or failed.
    #[cfg(feature = "avahi")]
    #[error("Avahi: {0}")]
    Avahi(#[from] zbus::Error),
    /// Listing the interfaces failed.
    #[error("listing network interfaces: {0}")]
    Interfaces(#[from] std::io::Error),
    /// No interface has this address.
    #[error("no interface has the address {0}")]
    NoInterface(IpAddr),
    /// The responder's feature is off.
    #[error("the {0} responder is not compiled in (feature {0})")]
    NotCompiledIn(&'static str),
    /// Neither responder started.
    #[error("no responder: in-process: {in_process}; Avahi: {avahi}")]
    NoResponder {
        /// Why the in-process one did not start
        in_process: Box<Error>,
        /// Why Avahi did not
        avahi: Box<Error>,
    },
}

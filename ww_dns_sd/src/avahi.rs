//! [AvahiResponder]: records registered with the system's Avahi daemon over D-Bus, the fallback where an in-process
//! responder can't run (port 5353 held without `SO_REUSEPORT`, a sandbox without multicast).

use std::collections::{BTreeMap, HashMap};
use std::net::IpAddr;

use zbus::blocking::Connection;
use zbus::zvariant::OwnedObjectPath;

use crate::{Error, InstanceName, Responder, SERVICE_TYPE, ServiceRecord};

/// `AVAHI_PROTO_INET`
const PROTO_INET: i32 = 0;
/// `AVAHI_PROTO_INET6`
const PROTO_INET6: i32 = 1;
/// `AVAHI_PROTO_UNSPEC`
const PROTO_UNSPEC: i32 = -1;
/// `AVAHI_PUBLISH_NO_REVERSE`: no PTR record for the address, which the machine's own host name owns
const PUBLISH_NO_REVERSE: u32 = 16;

#[zbus::proxy(
    interface = "org.freedesktop.Avahi.Server",
    default_service = "org.freedesktop.Avahi",
    default_path = "/",
    gen_async = false
)]
trait Server {
    fn entry_group_new(&self) -> zbus::Result<OwnedObjectPath>;
}

#[zbus::proxy(
    interface = "org.freedesktop.Avahi.EntryGroup",
    default_service = "org.freedesktop.Avahi",
    gen_async = false
)]
trait EntryGroup {
    #[expect(clippy::too_many_arguments, reason = "Avahi's D-Bus method")]
    fn add_service(
        &self,
        interface: i32,
        protocol: i32,
        flags: u32,
        name: &str,
        type_: &str,
        domain: &str,
        host: &str,
        port: u16,
        txt: Vec<Vec<u8>>,
    ) -> zbus::Result<()>;

    fn add_address(
        &self,
        interface: i32,
        protocol: i32,
        flags: u32,
        name: &str,
        address: &str,
    ) -> zbus::Result<()>;

    fn commit(&self) -> zbus::Result<()>;

    fn reset(&self) -> zbus::Result<()>;

    fn free(&self) -> zbus::Result<()>;
}

/// Registers records with Avahi over D-Bus, one entry group per instance. Avahi frees the groups by itself when
/// the connection closes, so a crashed server's record goes too.
pub struct AvahiResponder {
    conn: Connection,
    groups: HashMap<InstanceName, EntryGroupProxy<'static>>,
}

impl AvahiResponder {
    /// Connects to Avahi on the system bus.
    pub fn new() -> Result<Self, Error> {
        Self::with_connection(Connection::system()?)
    }

    /// Talks to Avahi, or to anything serving its interfaces, over `conn`.
    pub fn with_connection(conn: Connection) -> Result<Self, Error> {
        Ok(Self {
            conn,
            groups: HashMap::new(),
        })
    }

    fn group(&mut self, instance: &InstanceName) -> Result<&EntryGroupProxy<'static>, Error> {
        if !self.groups.contains_key(instance) {
            let path = ServerProxy::new(&self.conn)?.entry_group_new()?;
            let group = EntryGroupProxy::builder(&self.conn).path(path)?.build()?;
            self.groups.insert(instance.clone(), group);
        }
        Ok(self
            .groups
            .get(instance)
            .expect("inserted above when missing"))
    }
}

/// Index of the interface whose subnet holds `address`, as `mdns-sd` picks them.
fn interface_of(address: IpAddr, interfaces: &[if_addrs::Interface]) -> Result<i32, Error> {
    let index = interfaces
        .iter()
        .find(|i| in_subnet(address, &i.addr))
        .and_then(|i| i.index)
        .ok_or(Error::NoInterface(address))?;
    i32::try_from(index).map_err(|_| Error::NoInterface(address))
}

fn in_subnet(address: IpAddr, on: &if_addrs::IfAddr) -> bool {
    match (address, on) {
        (IpAddr::V4(a), if_addrs::IfAddr::V4(i)) => {
            let mask = u32::from(i.netmask);
            u32::from(a) & mask == u32::from(i.ip) & mask
        }
        (IpAddr::V6(a), if_addrs::IfAddr::V6(i)) => {
            let mask = u128::from(i.netmask);
            u128::from(a) & mask == u128::from(i.ip) & mask
        }
        _ => false,
    }
}

impl Responder for AvahiResponder {
    fn publish(&mut self, record: &ServiceRecord) -> Result<(), Error> {
        let interfaces = if_addrs::get_if_addrs()?;
        let mut by_interface: BTreeMap<i32, Vec<IpAddr>> = BTreeMap::new();
        for a in record.addresses() {
            by_interface
                .entry(interface_of(*a, &interfaces)?)
                .or_default()
                .push(*a);
        }
        let host = record.host_name();
        let name = record.instance().as_str();
        let port = record.port().get();
        let txt = record.txt().strings();
        let group = self.group(record.instance())?;
        group.reset()?;
        for (interface, addresses) in &by_interface {
            // announced over the address families it has addresses in, so a browser over the other one finds no
            // instance it can't resolve
            let v4 = addresses.iter().any(IpAddr::is_ipv4);
            let v6 = addresses.iter().any(IpAddr::is_ipv6);
            let protocol = match (v4, v6) {
                (true, false) => PROTO_INET,
                (false, true) => PROTO_INET6,
                _ => PROTO_UNSPEC,
            };
            group.add_service(
                *interface,
                protocol,
                0,
                name,
                SERVICE_TYPE,
                "",
                &host,
                port,
                txt.clone(),
            )?;
            for a in addresses {
                let protocol = if a.is_ipv4() { PROTO_INET } else { PROTO_INET6 };
                group.add_address(
                    *interface,
                    protocol,
                    PUBLISH_NO_REVERSE,
                    &host,
                    &a.to_string(),
                )?;
            }
        }
        group.commit()?;
        Ok(())
    }

    fn withdraw(&mut self, instance: &InstanceName) -> Result<(), Error> {
        match self.groups.remove(instance) {
            Some(group) => Ok(group.free()?),
            None => Ok(()),
        }
    }
}

//! The Avahi responder against a fake Avahi: the D-Bus calls that put a record up, change it and take it down.

use std::net::IpAddr;
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};

use ww_dns_sd::{Advertiser, AvahiResponder};
use zbus::{Connection, ObjectServer, OwnedObjectPath, block_on, connection};

use crate::common::{PORT, record};

#[derive(Debug, PartialEq)]
enum Call {
    EntryGroupNew,
    Reset(String),
    AddService {
        group: String,
        interface: i32,
        protocol: i32,
        name: String,
        type_: String,
        host: String,
        port: u16,
        txt: Vec<String>,
    },
    AddAddress {
        group: String,
        interface: i32,
        protocol: i32,
        flags: u32,
        host: String,
        address: String,
    },
    Commit(String),
    Free(String),
}

type Log = Arc<Mutex<Vec<Call>>>;

fn take(log: &Log) -> Vec<Call> {
    std::mem::take(&mut *log.lock().unwrap())
}

struct FakeServer {
    log: Log,
    groups: u32,
}

#[zbus::interface(name = "org.freedesktop.Avahi.Server")]
impl FakeServer {
    async fn entry_group_new(
        &mut self,
        #[zbus(object_server)] server: &ObjectServer,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        self.groups += 1;
        let path = format!("/Client1/EntryGroup{}", self.groups);
        self.log.lock().unwrap().push(Call::EntryGroupNew);
        let group = FakeGroup {
            log: self.log.clone(),
            path: path.clone(),
        };
        server.at(path.as_str(), group).await?;
        Ok(OwnedObjectPath::try_from(path).unwrap())
    }
}

struct FakeGroup {
    log: Log,
    path: String,
}

#[zbus::interface(name = "org.freedesktop.Avahi.EntryGroup")]
impl FakeGroup {
    #[expect(clippy::too_many_arguments, reason = "Avahi's D-Bus method")]
    fn add_service(
        &self,
        interface: i32,
        protocol: i32,
        _flags: u32,
        name: String,
        type_: String,
        _domain: String,
        host: String,
        port: u16,
        txt: Vec<Vec<u8>>,
    ) {
        self.log.lock().unwrap().push(Call::AddService {
            group: self.path.clone(),
            interface,
            protocol,
            name,
            type_,
            host,
            port,
            txt: txt
                .into_iter()
                .map(|t| String::from_utf8(t).unwrap())
                .collect(),
        });
    }

    fn add_address(
        &self,
        interface: i32,
        protocol: i32,
        flags: u32,
        name: String,
        address: String,
    ) {
        self.log.lock().unwrap().push(Call::AddAddress {
            group: self.path.clone(),
            interface,
            protocol,
            flags,
            host: name,
            address,
        });
    }

    fn commit(&self) {
        self.log
            .lock()
            .unwrap()
            .push(Call::Commit(self.path.clone()));
    }

    fn reset(&self) {
        self.log
            .lock()
            .unwrap()
            .push(Call::Reset(self.path.clone()));
    }

    async fn free(&self, #[zbus(object_server)] server: &ObjectServer) -> zbus::fdo::Result<()> {
        self.log.lock().unwrap().push(Call::Free(self.path.clone()));
        server.remove::<Self, _>(self.path.as_str()).await?;
        Ok(())
    }
}

/// A client connection to a fake Avahi on the other end of a socket pair.
fn fake_avahi() -> (Connection, Connection, Log) {
    let (a, b) = UnixStream::pair().unwrap();
    let log = Log::default();
    let server_log = log.clone();
    let server = std::thread::spawn(move || {
        block_on(
            connection::Builder::unix_stream(a)
                .server(zbus::Guid::generate())
                .p2p()
                .serve_at(
                    "/",
                    FakeServer {
                        log: server_log,
                        groups: 0,
                    },
                )
                .build(),
        )
        .unwrap()
    });
    let client = block_on(connection::Builder::unix_stream(b).p2p().build()).unwrap();
    (client, server.join().unwrap(), log)
}

/// The index of the interface with `address`.
fn interface_index(address: IpAddr) -> i32 {
    let i = if_addrs::get_if_addrs()
        .unwrap()
        .into_iter()
        .find(|i| i.ip() == address)
        .expect("loopback has 127.0.0.1");
    i.index.unwrap().try_into().unwrap()
}

fn service(group: &str, interface: i32, protocol: i32) -> Call {
    Call::AddService {
        group: group.to_owned(),
        interface,
        protocol,
        name: "Living room TV".to_owned(),
        type_: "_wireweaver._tcp".to_owned(),
        host: "ww-tv-2.local".to_owned(),
        port: PORT,
        txt: ["path=/ww", "api=tv_api@0.3.1", "wire=w1.2", "id=tv-2"]
            .map(str::to_owned)
            .to_vec(),
    }
}

fn address(group: &str, interface: i32, protocol: i32, address: &str) -> Call {
    Call::AddAddress {
        group: group.to_owned(),
        interface,
        protocol,
        flags: 16,
        host: "ww-tv-2.local".to_owned(),
        address: address.to_owned(),
    }
}

#[test]
fn avahi_gets_record_up_changed_and_down() {
    let (client, _server, log) = fake_avahi();
    let responder = AvahiResponder::with_connection(client).unwrap();
    let v4: IpAddr = "127.0.0.1".parse().unwrap();
    let second: IpAddr = "127.0.0.2".parse().unwrap();
    let lo = interface_index(v4);
    let g = "/Client1/EntryGroup1";

    let mut adv = Advertiser::new(responder, record("tv-2").with_addresses([v4]).unwrap()).unwrap();
    assert_eq!(
        take(&log),
        [
            Call::EntryGroupNew,
            Call::Reset(g.to_owned()),
            service(g, lo, 0),
            address(g, lo, 0, "127.0.0.1"),
            Call::Commit(g.to_owned()),
        ]
    );

    // the same entry group, emptied and filled again; 127.0.0.2 is in loopback's subnet
    assert!(adv.set_addresses([v4, second]).unwrap());
    assert_eq!(
        take(&log),
        [
            Call::Reset(g.to_owned()),
            service(g, lo, 0),
            address(g, lo, 0, "127.0.0.1"),
            address(g, lo, 0, "127.0.0.2"),
            Call::Commit(g.to_owned()),
        ]
    );

    drop(adv);
    assert_eq!(take(&log), [Call::Free(g.to_owned())]);
}

#[test]
fn address_on_no_interface_is_refused() {
    let (client, _server, log) = fake_avahi();
    let responder = AvahiResponder::with_connection(client).unwrap();
    // TEST-NET-1, on no interface of any machine
    let nowhere: IpAddr = "192.0.2.1".parse().unwrap();
    let r = Advertiser::new(responder, record("tv-2").with_addresses([nowhere]).unwrap());
    assert!(matches!(r, Err(ww_dns_sd::Error::NoInterface(a)) if a == nowhere));
    assert_eq!(take(&log), []);
}

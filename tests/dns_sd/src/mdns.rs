//! The in-process responder seen by an mDNS browser: the record appears, changes address and goes.

use std::net::{IpAddr, UdpSocket};
use std::time::Duration;

use mdns_sd::{IfKind, ResolvedService, ServiceDaemon, ServiceEvent};
use ww_dns_sd::{Advertiser, MdnsResponder};

use crate::common::{PORT, record};

/// Hang detector only: every wait ends on an event, normally within a second or two.
const EVENT_TIMEOUT: Duration = Duration::from_secs(20);

const TY: &str = "_wireweaver._tcp.local.";

/// A UDP port nothing on this machine uses, for both daemons: nothing outside the test sees their packets.
fn free_port() -> u16 {
    UdpSocket::bind("0.0.0.0:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn loopback_only(daemon: &ServiceDaemon) {
    daemon.disable_interface(IfKind::All).unwrap();
    daemon.enable_interface(IfKind::LoopbackV4).unwrap();
    daemon.set_multicast_loop_v4(true).unwrap();
}

/// The next resolved instance or removal, skipping search notices.
fn next(events: &mdns_sd::Receiver<ServiceEvent>) -> ServiceEvent {
    loop {
        match events.recv_timeout(EVENT_TIMEOUT).expect("an mDNS event") {
            ServiceEvent::SearchStarted(_) | ServiceEvent::ServiceFound(..) => continue,
            e => return e,
        }
    }
}

fn resolved(events: &mdns_sd::Receiver<ServiceEvent>) -> Box<ResolvedService> {
    match next(events) {
        ServiceEvent::ServiceResolved(r) => r,
        e => panic!("expected a resolved service, got {e:?}"),
    }
}

fn removed(events: &mdns_sd::Receiver<ServiceEvent>) -> String {
    match next(events) {
        ServiceEvent::ServiceRemoved(_, fullname) => fullname,
        e => panic!("expected a removal, got {e:?}"),
    }
}

fn addresses(r: &ResolvedService) -> Vec<IpAddr> {
    let mut list: Vec<IpAddr> = r.get_addresses().iter().map(|a| a.to_ip_addr()).collect();
    list.sort_unstable();
    list
}

#[test]
fn browser_sees_record_appear_change_and_go() {
    let port = free_port();
    let responder = MdnsResponder::with_port(port).unwrap();
    loopback_only(responder.daemon());
    let browser = ServiceDaemon::new_with_port(port).unwrap();
    loopback_only(&browser);
    let events = browser.browse(TY).unwrap();

    let first: IpAddr = "127.0.0.1".parse().unwrap();
    let mut adv =
        Advertiser::new(responder, record("tv-1").with_addresses([first]).unwrap()).unwrap();

    let r = resolved(&events);
    assert_eq!(r.get_fullname(), "Living room TV._wireweaver._tcp.local.");
    assert_eq!(r.get_hostname(), "ww-tv-1.local.");
    assert_eq!(r.get_port(), PORT);
    assert_eq!(addresses(&r), [first]);
    assert_eq!(r.get_property_val_str("path"), Some("/ww"));
    assert_eq!(r.get_property_val_str("api"), Some("tv_api@0.3.1"));
    assert_eq!(r.get_property_val_str("wire"), Some("w1.2"));
    assert_eq!(r.get_property_val_str("id"), Some("tv-1"));
    assert_eq!(r.get_properties().len(), 4);

    // a new lease: the old address goes with a goodbye, which the browser may report as a removal, and the
    // instance comes back with the new address alone
    let second: IpAddr = "127.0.0.2".parse().unwrap();
    assert!(adv.set_addresses([second]).unwrap());
    let mut seen = Vec::new();
    loop {
        assert!(seen.len() < 6, "the old address stays: {seen:?}");
        match next(&events) {
            ServiceEvent::ServiceResolved(r) => {
                assert_eq!(r.get_hostname(), "ww-tv-1.local.");
                assert_eq!(r.get_property_val_str("id"), Some("tv-1"));
                let now = addresses(&r);
                seen.push(format!("resolved {now:?}"));
                if now == [second] {
                    break;
                }
            }
            ServiceEvent::ServiceRemoved(_, name) => seen.push(format!("removed {name}")),
            e => panic!("unexpected {e:?}"),
        }
    }

    drop(adv);
    assert_eq!(removed(&events), "Living room TV._wireweaver._tcp.local.");
}

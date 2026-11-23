//! The in-process responder seen by an mDNS browser: the record appears, changes address and goes.

use std::net::{IpAddr, UdpSocket};
use std::time::Duration;

use mdns_sd::{IfKind, ResolvedService, ServiceDaemon, ServiceEvent};
use ww_dns_sd::{Advertiser, Browsed, Browser, MdnsOptions, MdnsResponder};

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

/// The next thing `browser` saw, which must come before the hang detector fires.
fn browsed(browser: &Browser) -> Browsed {
    browser
        .next(EVENT_TIMEOUT)
        .unwrap()
        .expect("a DNS-SD change")
}

#[test]
fn ww_browser_finds_the_record_follows_its_address_and_sees_it_go() {
    let options = MdnsOptions {
        port: free_port(),
        loopback_only: true,
    };
    let responder = MdnsResponder::with_options(&options).unwrap();
    let browser = Browser::with_options(&options).unwrap();

    let first: IpAddr = "127.0.0.1".parse().unwrap();
    let mut adv =
        Advertiser::new(responder, record("tv-1").with_addresses([first]).unwrap()).unwrap();

    let Browsed::Found(found) = browsed(&browser) else {
        panic!("expected the record to be found");
    };
    assert_eq!(found.instance(), "Living room TV");
    assert_eq!(found.port().get(), PORT);
    assert_eq!(found.path(), "/ww");
    assert_eq!(found.api(), Some("tv_api@0.3.1"));
    assert_eq!(found.api_crate(), Some("tv_api"));
    assert_eq!(found.wire(), Some("w1.2"));
    assert_eq!(found.id().map(|id| id.as_str()), Some("tv-1"));
    assert_eq!(found.addresses(), [first]);
    assert_eq!(found.websocket_urls(), ["ws://127.0.0.1:7590/ww"]);

    // a new lease: the goodbye of the old address may show as the server going, then it comes back at the new one
    let second: IpAddr = "127.0.0.2".parse().unwrap();
    assert!(adv.set_addresses([second]).unwrap());
    let mut seen = Vec::new();
    loop {
        assert!(seen.len() < 6, "the old address stays: {seen:?}");
        match browsed(&browser) {
            Browsed::Found(found) => {
                assert_eq!(found.instance(), "Living room TV");
                seen.push(format!("found {:?}", found.addresses()));
                if found.addresses() == [second] {
                    break;
                }
            }
            Browsed::Gone(name) => seen.push(format!("gone {name}")),
        }
    }

    drop(adv);
    assert_eq!(
        browsed(&browser),
        Browsed::Gone("Living room TV".to_owned())
    );
}

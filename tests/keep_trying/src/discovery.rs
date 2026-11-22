//! A keep-trying client of a server found over DNS-SD (CLT-82): the server announces itself on loopback mDNS, moves
//! to another address while the client is connected, and the reconnect finds it there. Real sockets and mDNS on a
//! port of its own, so it runs on the real clock; every wait ends on an event.

use std::net::{IpAddr, TcpListener, UdpSocket};
use std::num::NonZeroU16;
use std::time::Duration;

use tokio::sync::Notify;
use wire_weaver_client::discovery::{self, Discovery, FoundServer, MdnsOptions, Select};
use wire_weaver_client::{Backoff, ConnectionEvent, ConnectionState, KeepTrying};
use ww_dns_sd::{Advertiser, InstanceId, InstanceName, MdnsResponder, ServiceRecord, Txt};

use crate::tests::client::Client;
use crate::tests::{next_outcome, server};

/// Hang detector only: each wait ends on an event, normally within a second or two.
const EVENT_TIMEOUT: Duration = Duration::from_secs(20);

const NAME: &str = "Counter on the LAN";

/// A UDP port nothing on this machine uses: the mDNS of this test only.
fn free_udp_port() -> u16 {
    UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn start_device(listener: TcpListener) -> tests_common::TestDevice {
    tests_common::start_device_async_ws_tcp(
        listener,
        server::Server::new(),
        keep_trying_api::COUNTER_FULL_GID,
        keep_trying_api::COUNTER_WIRE_VERSION,
        tests_common::api!(server::api_impl),
    )
}

fn record(port: u16, at: IpAddr) -> ServiceRecord {
    let txt = Txt::new(
        "/ww",
        &keep_trying_api::COUNTER_FULL_GID,
        Some(keep_trying_api::COUNTER_WIRE_VERSION),
        InstanceId::new("counter-1").unwrap(),
    )
    .unwrap();
    ServiceRecord::new(
        InstanceName::new(NAME).unwrap(),
        NonZeroU16::new(port).unwrap(),
        txt,
    )
    .with_addresses([at])
    .unwrap()
}

/// Waits until `until` holds for `discovery`'s list, woken by `changed`.
async fn wait_for(discovery: &Discovery, changed: &Notify, until: impl Fn(&[FoundServer]) -> bool) {
    tokio::time::timeout(EVENT_TIMEOUT, async {
        loop {
            let notified = changed.notified();
            if until(&discovery.servers()) {
                return;
            }
            notified.await;
        }
    })
    .await
    .expect("discovery changes");
}

/// Waits until `discovery` lists the server at `at` only.
async fn found_at(discovery: &Discovery, changed: &Notify, at: IpAddr) {
    wait_for(discovery, changed, |servers| {
        servers
            .iter()
            .any(|s| s.instance() == NAME && s.addresses() == [at])
    })
    .await;
}

async fn outcome(
    events: &mut tokio::sync::broadcast::Receiver<ConnectionEvent>,
) -> ConnectionEvent {
    tokio::time::timeout(EVENT_TIMEOUT, next_outcome(events))
        .await
        .expect("a connection event")
}

#[tokio::test]
async fn a_discovered_server_is_followed_to_its_new_address() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let mdns = MdnsOptions {
                port: free_udp_port(),
                loopback_only: true,
            };
            let first: IpAddr = "127.0.0.1".parse().unwrap();
            let second: IpAddr = "127.0.0.2".parse().unwrap();
            let listener = TcpListener::bind((first, 0)).unwrap();
            let port = listener.local_addr().unwrap().port();
            // the same port at the second address, where the server moves to
            let moved = TcpListener::bind((second, port)).unwrap();

            let discovery = Discovery::start_with(&mdns).unwrap();
            let changed = std::sync::Arc::new(Notify::new());
            let notify = changed.clone();
            assert!(discovery.on_change(move || notify.notify_one()));
            let device = start_device(listener);
            let responder = MdnsResponder::with_options(&mdns).unwrap();
            let mut advertiser = Advertiser::new(responder, record(port, first)).unwrap();
            found_at(&discovery, &changed, first).await;

            let client = Client::config(|c| {
                c.websocket_urls_from(discovery::urls(
                    &discovery,
                    Select::Name(NAME.to_owned()),
                    Vec::new(),
                ))
                .resolver(tests_common::resolver())
                .keep_trying(KeepTrying {
                    backoff: Backoff {
                        first: Duration::from_millis(10),
                        max: Duration::from_millis(100),
                    },
                    give_up: None,
                })
                .clock_sync(false)
            })
            .connect_keep_trying()
            .expect("the handle is made at once");
            let mut events = client.cmd.connection_events();
            let state = client.cmd.connection_state().expect("keep-trying");
            match outcome(&mut events).await {
                ConnectionEvent::Connected { address, .. } => {
                    assert_eq!(address, format!("ws ws://{first}:{port}/ww"));
                }
                event => panic!("expected Connected, got {event:?}"),
            }

            // a new lease: the server goes from the old address and comes back at the new one
            drop(device);
            match outcome(&mut events).await {
                ConnectionEvent::Disconnected { reconnecting, .. } => assert!(reconnecting),
                event => panic!("expected Disconnected, got {event:?}"),
            }
            let _device = start_device(moved);
            assert!(advertiser.set_addresses([second]).unwrap());
            found_at(&discovery, &changed, second).await;
            match outcome(&mut events).await {
                ConnectionEvent::Reconnected { .. } => {}
                event => panic!("expected Reconnected, got {event:?}"),
            }
            assert_eq!(
                *state.borrow(),
                ConnectionState::Connected {
                    address: format!("ws ws://{second}:{port}/ww")
                }
            );
            let read = client.read_ticks().read().await.expect("calls go through");
            assert!(read.value > 0);

            // withdrawn: the server goes from the list
            drop(advertiser);
            wait_for(&discovery, &changed, <[FoundServer]>::is_empty).await;
        })
        .await
}

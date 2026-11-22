//! A keyed array whose key borrows (API-72): `keyed!(detector by DetectorName<'i>)`. The blocking `no_alloc` server
//! takes and gives `DetectorName<'_>`, the async server, the client, the local trait and `serve_trait` the owned
//! `DetectorNameOwned`; the key's bytes are the same whichever form wrote them.

use super::*;
use crate::local::{client::Client as LocalClient, server as proxy, traits::Dissect};
use futures_lite::StreamExt;
use keyed_api::{DetectorName, DetectorNameOwned};
use wire_weaver::CallContext;
use wire_weaver_client::in_process::TokioClock;
use wire_weaver_server::in_process::InProcessListener;
use wire_weaver_server::{Connection, Server};

// ids in the API: `detector[]` 3, `detector_keys` 4
const DETECTOR_KEYS: u32 = 4;

/// `"east"` as the key's shrink_wrap bytes: what a tape's `Segment::Key` hashes for the element (`swear/det/east`),
/// so they never change.
const EAST: [u8; 5] = [0x65, 0x61, 0x73, 0x74, 0x04];

fn name(s: &str) -> DetectorNameOwned {
    DetectorNameOwned(s.to_owned())
}

/// `Added { 3, "west" }` as the blocking server announces it, written from the borrowed key.
fn west_added() -> Vec<u8> {
    let event = KeyEvent::Added {
        index: UNib32(3),
        key: DetectorName("west"),
    };
    let mut scratch = [0u8; 128];
    blocking_server::api_impl::stream_data_ser()
        .detector_keys(&event, &mut scratch)
        .unwrap()
        .to_vec()
}

async fn connect_client(path: String) -> LocalClient {
    LocalClient::config(|c| {
        c.in_process_path(path.clone())
            .resolver(tests_common::resolver())
    })
    .connect()
    .await
    .expect("connect")
}

/// Borrowed and owned forms write the same bytes, and the device answers `key[2]` with them.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn key_bytes_are_stable() {
    let mut buf = [0u8; 16];
    assert_eq!(DetectorName("east").to_ww_bytes(&mut buf).unwrap(), EAST);
    assert_eq!(name("east").to_ww_bytes_owned().unwrap(), EAST);
    for device in [
        start_blocking("keyed/strings_bytes_blocking", table()),
        start_async("keyed/strings_bytes_async", table()),
    ] {
        let client = connect(&device).await;
        let answer = raw(
            client.cmd(),
            &[DETECTOR_KEYS, KEY, 2],
            RequestKindOwned::Read,
        )
        .await
        .unwrap();
        assert_eq!(answer, EAST);
    }
}

/// Every detector by its name from the generated client, against the borrowed and the owned server; an unknown
/// name is `None`; the cache learns an announced name from the `keys` stream.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_by_name() {
    for device in [
        start_blocking("keyed/strings_blocking", table()),
        start_async("keyed/strings_async", table()),
    ] {
        let client = connect_client(device.path()).await;
        for (index, (n, _)) in (0u32..).zip(DETECTORS) {
            assert_eq!(client.detector_by(&name(n)).await.unwrap(), Some(index));
            assert_eq!(client.detector_key(index).await.unwrap(), name(n));
        }
        assert_eq!(client.detector_index_of(&name("west")).await.unwrap(), None);

        let mut keys = client.detector_keys().await.unwrap();
        device.send(&west_added());
        assert_eq!(
            keys.recv().await.unwrap(),
            KeyEvent::Added {
                index: UNib32(3),
                key: name("west")
            }
        );
        // the device has no "west": the answer comes from the cache
        assert_eq!(client.detector_by(&name("west")).await.unwrap(), Some(3));
    }
}

/// The blocking client against the async server.
#[test]
fn client_by_name_blocking() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();
    let device = start_async("keyed/strings_client_blocking", table());
    let client = LocalClient::config(|c| {
        c.in_process_path(device.path())
            .resolver(tests_common::resolver())
    })
    .connect_blocking()
    .expect("connect");
    assert_eq!(
        client.detector_by_blocking(&name("south")).unwrap(),
        Some(1)
    );
    assert_eq!(client.detector_key_blocking(2).unwrap(), name("east"));
}

/// The client as a local `Dissect`, served again with `serve_trait`: a second client reaches the device through it,
/// each key crossing the trait as `DetectorNameOwned`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn local_trait_and_serve_trait() {
    let device = start_blocking("keyed/strings_upstream", table());
    let upstream = Arc::new(connect_client(device.path()).await);
    let cx = &CallContext::local();
    assert_eq!(
        Dissect::detector_index_of(&*upstream, cx, name("south"))
            .await
            .unwrap(),
        Some(1)
    );
    assert_eq!(
        Dissect::detector_key(&*upstream, cx, 2).await.unwrap(),
        name("east")
    );

    tests_common::api!(proxy::api_impl).seed();
    let path = "keyed/strings_proxy";
    let listener = InProcessListener::new(path, 4096);
    let (stop, stopped) = oneshot::channel::<()>();
    let served = upstream.clone();
    tokio::spawn(async move {
        let make = move |connection: &Connection| proxy::Server::new(served.clone(), connection);
        Server::new(TokioClock::new())
            .serve(proxy::link_config(), listener, make, async {
                let _ = stopped.await;
            })
            .await
            .expect("serve");
    });
    let client = connect_client(path.to_owned()).await;
    assert_eq!(client.detector_by(&name("north")).await.unwrap(), Some(0));
    assert_eq!(client.detector_key(1).await.unwrap(), name("south"));
    assert_eq!(client.detector_index_of(&name("west")).await.unwrap(), None);

    let mut keys = Dissect::detector_keys(&client, cx).await.unwrap();
    device.send(&west_added());
    assert_eq!(
        keys.next().await.unwrap().unwrap(),
        KeyEvent::Added {
            index: 3,
            key: name("west")
        }
    );
    drop(stop);
}

/// `ww api scaffold` names the borrowed key without an allocator and its owned form with one.
#[test]
fn scaffold_names_the_key_form() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../keyed_api");
    let bundle = wire_weaver_core::load(&path, Some("Dissector".into()), false).unwrap();
    for (no_alloc, key) in [
        (true, "_key: keyed_api :: DetectorName < '_ >,"),
        (false, "_key: keyed_api :: DetectorNameOwned,"),
    ] {
        let config = wire_weaver_core::ServerScaffoldConfig {
            server_struct: "Server".into(),
            no_alloc,
            use_async: false,
            handler_model: String::new(),
            receiver_model: String::new(),
            method_model: String::new(),
            property_model: String::new(),
            const_model: String::new(),
            medium: None,
            omit: vec![],
            observe_cells: vec![],
            observe_samples: true,
            introspect: String::new(),
            introspect_compression: String::new(),
        };
        let code = wire_weaver_core::gen_server_scaffold(
            &bundle,
            &config,
            &wire_weaver_core::CratePaths::default(),
        )
        .unwrap();
        let code: String = code.split_whitespace().collect::<Vec<_>>().join(" ");
        let getter = key.trim_start_matches("_key: ").trim_end_matches(',');
        for expected in [
            format!(
                "fn detector_index_of(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>, {key} ) -> RpcResult<Option < wire_weaver :: shrink_wrap :: UNib32 >>"
            ),
            format!(
                "fn get_detector_key(&mut self, _cx: &mut Context<'_, impl BlockingEventOut>, _index_chain: [ww_client_server::ResourceIndex; 1], ) -> GetResult<{getter}, CommonError<'_>>"
            ),
        ] {
            assert!(code.contains(&expected), "{expected}\n{code}");
        }
    }
}

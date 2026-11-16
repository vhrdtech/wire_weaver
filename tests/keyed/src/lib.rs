//! Server of a keyed array (SRV-62): the Dissector of `keyed_api` answers `index_of`, `key[i]`, a multi read of all
//! keys and the `keys` stream, from a blocking `no_alloc` server and an async one, through the host event loop, to
//! requests built by hand. The generated and dynamic clients (CLT-72) address flows a device created in shuffled
//! order by their keys.

pub mod device;

#[cfg(test)]
mod tests {
    use crate::device::*;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use keyed_api::{Endpoint, FlowKey, Proto};
    use tests_common::TestDevice;
    use tokio::sync::oneshot;
    use wire_weaver::prelude::*;
    use wire_weaver::shrink_wrap::either_any_vec::EitherAnyVec;
    use wire_weaver::shrink_wrap::tail_bytes::TailBytesOwned;
    use wire_weaver::shrink_wrap::{BufReader, UVlq32Backfill};
    use wire_weaver_client::internal::Command;
    use wire_weaver_client::{ClientConfig, Commander, DynClient, DynResource, Error};
    use ww_client_server::{
        ErrorKindOwned, ErrorOwned, MultiIndexOwned, PathKind, PathKindOwned, RequestKindOwned,
        RequestOwned, ResourceIndex,
    };
    use ww_keyed::KeyEvent;

    /// Same wire format as the generated arguments of `index_of`.
    #[derive_shrink_wrap(owned)]
    struct IndexOfArgs {
        key: FlowKey,
    }

    // ids in the API: `flow[]` 0, `flow_keys` 1, `clear` 2; the companion's level: `index_of` 0, `key[]` 1, `keys` 2
    const FLOW_KEYS: u32 = 1;
    const CLEAR: u32 = 2;
    const INDEX_OF: u32 = 0;
    const KEY: u32 = 1;
    const KEYS: u32 = 2;

    async fn connect(device: &TestDevice) -> DynClient {
        DynClient::from_config(
            ClientConfig::new()
                .in_process_path(device.path())
                .resolver(tests_common::resolver()),
        )
        .connect()
        .await
        .expect("connect")
    }

    fn resource_path(path: &[u32]) -> Vec<ResourceIndex> {
        path.iter()
            .copied()
            .map(|i| ResourceIndex::new(UNib32(i)))
            .collect()
    }

    /// Send a request as is and wait for the answer: the value bytes, or the device's error.
    async fn raw(cmd: &Commander, path: &[u32], kind: RequestKindOwned) -> Result<Vec<u8>, Error> {
        let req = RequestOwned {
            seq: UVlq32Backfill(0),
            priority: ww_client_server::Priority::NORMAL.to_wire(),
            reserved: false,
            path_kind: PathKindOwned::Absolute {
                path: resource_path(path),
            },
            kind,
        };
        let bytes = req.to_ww_bytes_owned().unwrap();
        let (tx, rx) = oneshot::channel();
        cmd.send(Command::SendMessage {
            bytes,
            done_tx: Some((tx, Duration::from_secs(2))),
        })
        .await?;
        rx.await.expect("event loop answers")
    }

    async fn index_of(cmd: &Commander, key: &FlowKey) -> Option<UNib32> {
        let args = IndexOfArgs { key: key.clone() }
            .to_ww_bytes_owned()
            .unwrap();
        let kind = RequestKindOwned::Call {
            args: TailBytesOwned(args),
        };
        let answer = raw(cmd, &[FLOW_KEYS, INDEX_OF], kind).await.unwrap();
        Option::<UNib32>::from_ww_bytes(&answer).unwrap()
    }

    /// `index_of`, `key[i]` and the key array's valid indices, `key` of a free index refused.
    async fn index_of_and_key(cmd: &Commander) {
        assert_eq!(index_of(cmd, &key(5001, 20)).await, Some(UNib32(2)));
        assert_eq!(index_of(cmd, &key(5000, 10)).await, Some(UNib32(0)));
        assert_eq!(index_of(cmd, &key(5001, 21)).await, None);

        let answer = raw(cmd, &[FLOW_KEYS, KEY, 3], RequestKindOwned::Read)
            .await
            .unwrap();
        assert_eq!(
            FlowKey::from_ww_bytes_owned(&answer).unwrap(),
            key(5002, 30)
        );

        let refused = raw(cmd, &[FLOW_KEYS, KEY, 1], RequestKindOwned::Read).await;
        assert!(
            matches!(
                refused,
                Err(Error::RemoteError(ErrorOwned {
                    kind: ErrorKindOwned::BadIndex,
                    ..
                }))
            ),
            "{refused:?}"
        );

        let answer = raw(cmd, &[FLOW_KEYS, KEY], RequestKindOwned::Read)
            .await
            .unwrap();
        let indices = ValidIndicesOwned::from_ww_bytes_owned(&answer).unwrap();
        assert_eq!(indices.iter().collect::<Vec<_>>(), [0, 2, 3]);
    }

    /// Every key in one multi read over the valid indices.
    async fn multi_read_keys(cmd: &Commander) {
        let kind = RequestKindOwned::MultiRead {
            multi_idx: MultiIndexOwned::List(resource_path(&[0, 2, 3])),
            in_each_array_id: None,
        };
        let answer = raw(cmd, &[FLOW_KEYS, KEY], kind).await.unwrap();
        let mut rd = BufReader::new(&answer);
        let vec = EitherAnyVec::des_shrink_wrap(&mut rd).unwrap();
        let mut iter = vec.iter();
        let keys: Vec<FlowKey> = (0..3)
            .map(|_| {
                let item: Result<FlowKey, ErrorOwned> =
                    iter.next_owned::<ErrorOwned, FlowKey>().unwrap().into();
                item.unwrap()
            })
            .collect();
        assert_eq!(keys, [key(5000, 10), key(5001, 20), key(5002, 30)]);
    }

    /// The `keys` stream: opened without a handler, an `Added` sent from outside a handler, `Removed` for each flow
    /// from `clear`'s handler.
    async fn keys_stream(device: &TestDevice, cmd: &Commander, added: &[u8]) {
        let path = resource_path(&[FLOW_KEYS, KEYS]);
        let mut stream = cmd
            .prepare_stream::<KeyEvent<UNib32, FlowKey>>(PathKind::Absolute {
                path: RefVec::Slice { slice: &path },
            })
            .await
            .unwrap();
        let open = RequestKindOwned::StreamOpen {
            resume: None,
            max_age: None,
            raw: false,
            args: TailBytesOwned(vec![]),
        };
        raw(cmd, &[FLOW_KEYS, KEYS], open).await.unwrap();

        device.send(added);
        assert_eq!(
            stream.recv().await.unwrap(),
            KeyEvent::Added {
                index: UNib32(4),
                key: key(5003, 40)
            }
        );

        let clear = RequestKindOwned::Call {
            args: TailBytesOwned(vec![]),
        };
        raw(cmd, &[CLEAR], clear).await.unwrap();
        for index in [0, 2, 3] {
            assert_eq!(
                stream.recv().await.unwrap(),
                KeyEvent::Removed {
                    index: UNib32(index)
                }
            );
        }
        let refused = raw(cmd, &[FLOW_KEYS, KEY, 0], RequestKindOwned::Read).await;
        assert!(refused.is_err(), "no flow after clear: {refused:?}");
    }

    /// `KeyEvent::Added { 4, key(5003, 40) }` as the device sends it on the `keys` stream.
    fn added_event(ser: impl FnOnce(&KeyEvent<UNib32, FlowKey>, &mut [u8]) -> Vec<u8>) -> Vec<u8> {
        let event = KeyEvent::Added {
            index: UNib32(4),
            key: key(5003, 40),
        };
        let mut scratch = [0u8; 128];
        ser(&event, &mut scratch)
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn blocking_no_alloc_server() {
        let device = start_blocking("keyed/blocking", table());
        let client = connect(&device).await;
        index_of_and_key(client.cmd()).await;
        multi_read_keys(client.cmd()).await;
        let added = added_event(|event, scratch| {
            blocking_server::api_impl::stream_data_ser()
                .flow_keys(event, scratch)
                .unwrap()
                .to_vec()
        });
        keys_stream(&device, client.cmd(), &added).await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn async_alloc_server() {
        let device = start_async("keyed/async", table());
        let client = connect(&device).await;
        index_of_and_key(client.cmd()).await;
        multi_read_keys(client.cmd()).await;
        let added = added_event(|event, scratch| {
            async_server::api_impl::stream_data_ser()
                .flow_keys(event, scratch)
                .unwrap()
                .to_vec()
        });
        keys_stream(&device, client.cmd(), &added).await;
    }

    /// The `keys` stream's events carry the companion's path: `flow_keys` 1, `keys` 2.
    #[test]
    fn keys_stream_path() {
        let added = added_event(|event, scratch| {
            blocking_server::api_impl::stream_data_ser()
                .flow_keys(event, scratch)
                .unwrap()
                .to_vec()
        });
        let event = ww_client_server::Event::from_ww_bytes(&added).unwrap();
        let ww_client_server::EventKind::StreamData { path, .. } = event.result.unwrap() else {
            panic!("not stream data");
        };
        let path: Vec<u32> = path.iter().map(|p| p.unwrap().get().0).collect();
        assert_eq!(path, [FLOW_KEYS, KEYS]);
    }

    mod std_client {
        use wire_weaver_client::{ClientConfig, Commander, WwClient};

        pub struct Client {
            pub cmd: Commander,
        }

        impl WwClient for Client {
            fn default_config() -> ClientConfig {
                ClientConfig::default()
            }

            fn try_from_cmd(cmd: Commander) -> Result<Self, wire_weaver_client::Error> {
                Ok(Self { cmd })
            }
        }

        mod api_client {
            wire_weaver::ww_codegen!(
                keyed_api :: Dissector for super::Client,
                client = "std_client",
                debug_to_file = "../../target/tests_keyed_client.rs"
            );
        }
    }

    use std_client::Client;

    /// Counts the `index_of` calls that reach the device.
    fn count_index_of(device: &TestDevice) -> Arc<std::sync::atomic::AtomicU32> {
        use std::sync::atomic::{AtomicU32, Ordering};
        let calls = Arc::new(AtomicU32::new(0));
        let counter = calls.clone();
        device.filter_requests(move |bytes| {
            if let Ok(req) = ww_client_server::Request::from_ww_bytes(bytes)
                && let ww_client_server::RequestKind::Call { .. } = req.kind
                && let PathKind::Absolute { path } = req.path_kind
                && path
                    .iter()
                    .map(|p| p.unwrap().get().0)
                    .eq([FLOW_KEYS, INDEX_OF])
            {
                counter.fetch_add(1, Ordering::Relaxed);
            }
            false
        });
        calls
    }

    fn calls(counter: &std::sync::atomic::AtomicU32) -> u32 {
        counter.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Every flow by its key, whatever index the device gave it; a removed one is `None`. Asked once per key.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn client_by_key() {
        let device = start_blocking("keyed/client", shuffled_table());
        let index_of = count_index_of(&device);
        let client = Client::config(|c| {
            c.in_process_path(device.path())
                .resolver(tests_common::resolver())
        })
        .connect()
        .await
        .expect("connect");
        for round in 0..2 {
            for i in 0..FLOWS {
                let (key, bytes) = flow(i);
                let found = client.flow_by(&key).await.unwrap();
                if i < REMOVED {
                    assert!(found.is_none(), "flow {i} was removed");
                    continue;
                }
                let handle = found.expect("a live flow");
                assert_eq!(handle.read_bytes().read().await.unwrap().get(), bytes);
                let index = handle.index_chain[1].get().0;
                assert_eq!(client.flow_key(index).await.unwrap(), key);
            }
            // a removed key is asked again each time, a found one only once
            assert_eq!(
                calls(&index_of),
                u32::from(FLOWS) + u32::from(REMOVED) * round
            );
        }
    }

    /// The blocking client against the async server, same as [client_by_key].
    #[test]
    fn client_by_key_blocking() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let device = start_async("keyed/client_blocking", shuffled_table());
        let index_of = count_index_of(&device);
        let client = Client::config(|c| {
            c.in_process_path(device.path())
                .resolver(tests_common::resolver())
        })
        .connect_blocking()
        .expect("connect");
        for _ in 0..2 {
            for i in REMOVED..FLOWS {
                let (key, bytes) = flow(i);
                let handle = client.flow_by_blocking(&key).unwrap().expect("a live flow");
                assert_eq!(handle.read_bytes().blocking_read().unwrap().get(), bytes);
                let index = handle.index_chain[1].get().0;
                assert_eq!(client.flow_key_blocking(index).unwrap(), key);
            }
        }
        assert_eq!(calls(&index_of), u32::from(FLOWS - REMOVED));
        let refused = client.flow_key_blocking(1000);
        assert!(
            matches!(
                refused,
                Err(Error::RemoteError(ErrorOwned {
                    kind: ErrorKindOwned::BadIndex,
                    ..
                }))
            ),
            "{refused:?}"
        );
    }

    /// The cache learns from the `keys` stream and forgets on `Removed` and on a `BadIndex` answer.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn cache_follows_the_device() {
        let table = shuffled_table();
        let device = start_blocking("keyed/cache", table.clone());
        let index_of = count_index_of(&device);
        let client = Client::config(|c| {
            c.in_process_path(device.path())
                .resolver(tests_common::resolver())
        })
        .connect()
        .await
        .expect("connect");
        let mut keys = client.flow_keys().await.unwrap();

        // announced: found without asking
        let (new, bytes) = (key(7000, 1), 99);
        let index = {
            let mut table = table.lock().unwrap();
            table.flows.push(Some((new.clone(), bytes)));
            table.flows.len() as u32 - 1
        };
        let added = KeyEvent::Added {
            index: UNib32(index),
            key: new.clone(),
        };
        let mut scratch = [0u8; 128];
        device.send(
            blocking_server::api_impl::stream_data_ser()
                .flow_keys(&added, &mut scratch)
                .unwrap(),
        );
        assert_eq!(keys.recv().await.unwrap(), added);
        let handle = client.flow_by(&new).await.unwrap().expect("announced");
        assert_eq!(handle.read_bytes().read().await.unwrap().get(), bytes);
        assert_eq!(calls(&index_of), 0);

        // removed: asked again
        table.lock().unwrap().flows[index as usize] = None;
        let removed = KeyEvent::Removed {
            index: UNib32(index),
        };
        device.send(
            blocking_server::api_impl::stream_data_ser()
                .flow_keys(&removed, &mut scratch)
                .unwrap(),
        );
        assert_eq!(keys.recv().await.unwrap(), removed);
        assert!(client.flow_by(&new).await.unwrap().is_none());
        assert_eq!(calls(&index_of), 1);

        // gone without a word: the element's BadIndex forgets it
        let (live, _) = flow(REMOVED);
        let handle = client.flow_by(&live).await.unwrap().expect("a live flow");
        assert_eq!(calls(&index_of), 2);
        let at = handle.index_chain[1].get().0 as usize;
        table.lock().unwrap().flows[at] = None;
        let refused = handle.read_bytes().read().await;
        assert!(
            matches!(
                refused,
                Err(Error::RemoteError(ErrorOwned {
                    kind: ErrorKindOwned::BadIndex,
                    ..
                }))
            ),
            "{refused:?}"
        );
        assert!(client.flow_by(&live).await.unwrap().is_none());
        assert_eq!(calls(&index_of), 3);
    }

    /// `flow(i)`'s key as a Rust expression, as `ww` and the dynamic client take it in a path.
    fn key_text(i: u16) -> String {
        let (key, _) = flow(i);
        let proto = match key.proto {
            Proto::Tcp => "Proto::Tcp",
            Proto::Udp => "Proto::Udp",
            Proto::Unknown(_) => unreachable!("the test's keys"),
        };
        let endpoint = |e: &Endpoint| {
            format!(
                "Endpoint {{ addr: {:?}, port: Port({}) }}",
                e.addr, e.port.0
            )
        };
        format!(
            "FlowKey {{ proto: {proto}, src: {}, dst: {}, first_seen: Point {{ coord: {} }} }}",
            endpoint(&key.src),
            endpoint(&key.dst),
            key.first_seen.raw()
        )
    }

    /// Index the device gave `flow(i)`.
    fn device_index(table: &Mutex<Table>, i: u16) -> u32 {
        let table = table.lock().unwrap();
        let at = table
            .flows
            .iter()
            .position(|f| f.as_ref().map(|(k, _)| k) == Some(&flow(i).0))
            .expect("a live flow");
        at as u32
    }

    /// The dynamic client finds flows by key in paths (`flow[FlowKey { .. }].bytes`), asks once per key, and
    /// reaches the companion's own level (`flow_keys.key[i]`).
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn dynamic_by_key() {
        let table = shuffled_table();
        let device = start_blocking("keyed/dynamic", table.clone());
        let index_of = count_index_of(&device);
        let client = connect(&device).await;
        let cmd = client.cmd();
        let root = DynResource::root(client.device_introspect().unwrap().api_bundle.clone());
        assert!(root.child("flow").unwrap().key_ty().is_some());
        for _ in 0..2 {
            for i in REMOVED..FLOWS {
                let res = root
                    .walk_on(cmd, &format!("flow[{}].bytes", key_text(i)))
                    .await
                    .unwrap();
                assert_eq!(res.index_chain()[1].get().0, device_index(&table, i));
                res.read(cmd).await.unwrap();
            }
        }
        assert_eq!(calls(&index_of), u32::from(FLOWS - REMOVED));

        let removed = root.walk_on(cmd, &format!("flow[{}]", key_text(0))).await;
        assert!(removed.unwrap_err().to_string().contains("no element"));
        let offline = root.walk(&format!("flow[{}].bytes", key_text(REMOVED)));
        assert!(offline.unwrap_err().to_string().contains("connect"));

        let i = REMOVED + 1;
        let index = device_index(&table, i);
        let key_value = root
            .walk(&format!("flow_keys.key[{index}]"))
            .unwrap()
            .read(cmd)
            .await
            .unwrap();
        let flow_array = root.child("flow").unwrap();
        let expected = wire_weaver_client::expr::parse_value(
            &key_text(i),
            flow_array.key_ty().unwrap(),
            flow_array.bundle(),
        )
        .unwrap();
        assert_eq!(key_value, expected);
    }

    /// The blocking dynamic client, and its cache following the `keys` stream opened by path.
    #[test]
    fn dynamic_by_key_blocking() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let _guard = rt.enter();
        let table = shuffled_table();
        let device = start_async("keyed/dynamic_blocking", table.clone());
        let index_of = count_index_of(&device);
        let client = DynClient::from_config(
            ClientConfig::new()
                .in_process_path(device.path())
                .resolver(tests_common::resolver()),
        )
        .connect_blocking()
        .expect("connect");
        let cmd = client.cmd();
        let root = DynResource::root(client.device_introspect().unwrap().api_bundle.clone());
        for i in REMOVED..FLOWS {
            let res = root
                .walk_on_blocking(cmd, &format!("flow[{}]", key_text(i)))
                .unwrap();
            assert_eq!(res.index_chain()[1].get().0, device_index(&table, i));
        }
        assert_eq!(calls(&index_of), u32::from(FLOWS - REMOVED));

        let mut keys = root
            .walk("flow_keys.keys")
            .unwrap()
            .stream_blocking(cmd)
            .unwrap();
        let (new, bytes) = (key(5000, 0), 5);
        let index = {
            let mut table = table.lock().unwrap();
            table.flows.push(Some((new.clone(), bytes)));
            table.flows.len() as u32 - 1
        };
        let added = KeyEvent::Added {
            index: UNib32(index),
            key: new,
        };
        let mut scratch = [0u8; 128];
        device.send(
            async_server::api_impl::stream_data_ser()
                .flow_keys(&added, &mut scratch)
                .unwrap(),
        );
        keys.recv_blocking().unwrap();
        let text = "FlowKey { proto: Proto::Tcp, src: Endpoint { addr: [10, 0, 0, 1], port: Port(5000) }, \
                    dst: Endpoint { addr: [10, 0, 0, 2], port: Port(443) }, first_seen: Point { coord: 0 } }";
        let res = root
            .walk_on_blocking(cmd, &format!("flow[{text}]"))
            .unwrap();
        assert_eq!(res.index_chain()[1].get().0, index);
        assert_eq!(
            calls(&index_of),
            u32::from(FLOWS - REMOVED),
            "announced, not asked"
        );
    }
}
